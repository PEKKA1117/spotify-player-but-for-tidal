//! The library: lists, pages and writes (spec 0006). Requests are answered
//! through the [`Authenticator`]; every list and page has a pure DTO to core
//! mapping behind it.
//!
//! Choices the spec leaves open:
//!
//! - **The artist's albums** are the plain list, then EPs and singles. Both
//!   are paged by the page size asked: the plain list takes the offsets
//!   below `ceil(total / limit) * limit`, the EPs and singles the offsets
//!   after, counted from 0. A plain page costs a second request, for the
//!   EP total only (`limit=1`), so `total` is always the sum.
//! - **The credits' `dataApiPath`** is read from `/pages/contributor` on
//!   `offset` 0 and kept per artist in the client; later pages use it, and
//!   read it again once when it fails with a `404`.
//! - **A credits page smaller than Tidal's 50** keeps the first rows of
//!   what Tidal sent.
//! - **A playlist's `own`** in its header is `creator.id == user_id`.
//! - **A playlist edit** that answers `412` is retried once per request to
//!   Tidal's `items` endpoint (one per chunk of 100 tracks).
//! - **A search's top hit** whose `value` cannot be read is no top hit:
//!   the lists still show (spec 0007).
//! - **A search's Atmos-only tracks** are dropped without counting them as
//!   `hidden`: the window's title shows Tidal's total, as 0006's favorite
//!   tracks do (spec 0007 "Edge cases").
//! - **A search `400` without a `userMessage`** reads as any other status
//!   (`Tidal answered 400`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use reqwest::Method;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use tidal_player_core::library::{
    AlbumKind, AlbumSummary, CreditedTrack, FavoriteKind, LibraryRequest, LibraryResponse,
    ListItems, ListPage, ListRef, PageData, PageRequest, PlaylistSummary, RoleCategory, TopHit,
    hidden_version,
};
use tidal_player_core::{ArtistRef, Item, Track, TrackId};

use crate::ApiResponse;
use crate::auth::{AuthError, Authenticator};
use crate::metadata::{ArtistDto, REQUEST_TIMEOUT, TrackDto};

/// Tidal's `subStatus` for an unknown item (on a `404`).
const SUB_STATUS_NOT_FOUND: u64 = 2001;

/// The largest page of the playlists endpoint and of the credits.
const LARGEST_SMALL: u32 = 50;
/// The largest page of a playlist's items.
const LARGEST_ITEMS: u32 = 100;
/// The largest page of every other list.
const LARGEST: u32 = 1000;
/// Tracks per request to a playlist's `items` (Tidal's limit).
const ADD_CHUNK: usize = 100;
/// The kinds a search asks for: always sent, or `videos` fill (spec 0007).
const SEARCH_TYPES: &str = "TRACKS,ALBUMS,ARTISTS,PLAYLISTS";

/// What was not found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Subject {
    Item(Item),
    Artist(u64),
}

impl std::fmt::Display for Subject {
    /// `Album 1`, `Playlist <uuid>`, `Track 1`, `Artist 1`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Item(item) => write!(f, "{item}"),
            Self::Artist(id) => write!(f, "Artist {id}"),
        }
    }
}

/// Why a library request failed. `Display` is the message a user sees.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LibraryError {
    /// `404` with `subStatus` `2001` on the request's own resource.
    #[error("{0} was not found")]
    NotFound(Subject),
    /// A `2xx` body that could not be read; never quotes the body.
    #[error("malformed {0} response from Tidal")]
    Malformed(&'static str),
    /// `412` on a playlist edit (a second one, for an add).
    #[error("The playlist changed: nothing was changed, try again")]
    PlaylistChanged,
    /// A track, album or artist ID that is not a number.
    #[error("Not a valid ID: {0}")]
    InvalidId(String),
    /// `400` on a search, with Tidal's `userMessage` (spec 0007).
    #[error("Tidal refused the search: {0}")]
    SearchRefused(String),
    /// From the [`Authenticator`], unchanged: `LoginRequired`, transport
    /// errors, unexpected statuses (`5xx`, `429`, ...).
    #[error("{}", auth_message(.0))]
    Auth(AuthError),
}

/// The user-facing wording of an [`AuthError`] met while browsing.
fn auth_message(error: &AuthError) -> String {
    match error {
        AuthError::Http { status: 429, .. } => {
            "Tidal answered 429: try again in a moment".to_owned()
        }
        AuthError::Http { status, .. } => format!("Tidal answered {status}"),
        AuthError::Transport(message) => format!("Could not reach Tidal: {message}"),
        other => other.to_string(),
    }
}

impl LibraryError {
    /// The underlying [`AuthError`]: `LoginRequired` and the transient
    /// errors (see [`AuthError::is_transient`]) are told apart through it.
    pub fn auth(&self) -> Option<&AuthError> {
        match self {
            Self::Auth(error) => Some(error),
            _ => None,
        }
    }
}

impl From<AuthError> for LibraryError {
    fn from(error: AuthError) -> Self {
        Self::Auth(error)
    }
}

type Result<T> = std::result::Result<T, LibraryError>;

/// Reads and writes the library through the [`Authenticator`].
#[derive(Debug, Clone)]
pub struct LibraryClient {
    auth: Arc<Authenticator>,
    /// The credits' `dataApiPath` per artist ID.
    credit_paths: Arc<Mutex<HashMap<u64, String>>>,
}

impl LibraryClient {
    pub fn new(auth: Arc<Authenticator>) -> Self {
        Self {
            auth,
            credit_paths: Arc::default(),
        }
    }

    /// Answers one request. `page_size` (1-10 000) is the size of the first
    /// page of each list in a `Page` request; `More` carries its own
    /// `limit`. Each request is clamped to the endpoint's largest page.
    /// `hidden_words` is the hidden-version word list of the credits.
    pub async fn request(
        &self,
        request: LibraryRequest,
        page_size: u32,
        hidden_words: &[String],
    ) -> Result<LibraryResponse> {
        match request {
            LibraryRequest::Page(page) => {
                self.page(page, page_size).await.map(LibraryResponse::Page)
            }
            LibraryRequest::More {
                list,
                offset,
                limit,
            } => self
                .list(&list, offset, limit, hidden_words)
                .await
                .map(LibraryResponse::Items),
            LibraryRequest::IsFavorite(kind, id) => self
                .is_favorite(kind, &id)
                .await
                .map(LibraryResponse::Favorite),
            LibraryRequest::AddFavorite(kind, id) => {
                self.add_favorite(kind, &id).await?;
                Ok(LibraryResponse::Done)
            }
            LibraryRequest::RemoveFavorite(kind, id) => {
                self.remove_favorite(kind, &id).await?;
                Ok(LibraryResponse::Done)
            }
            LibraryRequest::AddToPlaylist {
                uuid,
                tracks,
                allow_duplicates,
            } => {
                self.add_to_playlist(&uuid, &tracks, allow_duplicates)
                    .await?;
                Ok(LibraryResponse::Done)
            }
            LibraryRequest::RemoveFromPlaylist { uuid, index, etag } => {
                self.remove_from_playlist(&uuid, index, &etag).await?;
                Ok(LibraryResponse::Done)
            }
            LibraryRequest::CreatePlaylist { title } => self
                .create_playlist(&title)
                .await
                .map(LibraryResponse::Created),
            LibraryRequest::DeletePlaylist { uuid } => {
                self.delete_playlist(&uuid).await?;
                Ok(LibraryResponse::Done)
            }
        }
    }

    // Pages -----------------------------------------------------------

    /// The header and the first page of each list, from parallel requests;
    /// any failing call fails the page.
    async fn page(&self, request: PageRequest, size: u32) -> Result<PageData> {
        match request {
            PageRequest::Library => {
                let (playlists, albums, artists) = tokio::try_join!(
                    self.playlists(0, size),
                    self.favorite_albums(0, size),
                    self.favorite_artists(0, size),
                )?;
                Ok(PageData::Library {
                    playlists,
                    albums,
                    artists,
                })
            }
            PageRequest::FavoriteTracks => Ok(PageData::FavoriteTracks {
                tracks: self.favorite_tracks(0, size).await?,
            }),
            PageRequest::Album(id) => {
                let (album, tracks) =
                    tokio::try_join!(self.album_header(id), self.album_tracks(id, 0, size))?;
                Ok(PageData::Album { album, tracks })
            }
            PageRequest::Playlist(uuid) => {
                let ((playlist, etag), tracks) = tokio::try_join!(
                    self.playlist_header(&uuid),
                    self.playlist_tracks(&uuid, 0, size)
                )?;
                Ok(PageData::Playlist {
                    playlist,
                    etag,
                    tracks,
                })
            }
            PageRequest::Artist(id) => {
                let (artist, top_tracks, albums, appears_on) = tokio::try_join!(
                    self.artist_header(id),
                    self.top_tracks(id, 0, size),
                    self.artist_albums(id, 0, size),
                    self.appears_on(id, 0, size),
                )?;
                Ok(PageData::Artist {
                    artist,
                    top_tracks,
                    albums,
                    appears_on,
                })
            }
            PageRequest::Search(query) => self.search(&query, size).await,
        }
    }

    async fn album_header(&self, id: u64) -> Result<AlbumSummary> {
        let dto: AlbumDto = self
            .read(
                &format!("albums/{id}"),
                &[],
                Some(Subject::Item(Item::Album(id))),
                "album",
            )
            .await?;
        Ok(dto.into())
    }

    /// The playlist, and its `ETag` header.
    async fn playlist_header(&self, uuid: &str) -> Result<(PlaylistSummary, Option<String>)> {
        let (user, _) = self.auth.account().await;
        let response = self
            .get(
                &format!("playlists/{uuid}"),
                &[],
                Some(Subject::Item(Item::Playlist(uuid.to_owned()))),
            )
            .await?;
        let etag = response.header("etag").map(str::to_owned);
        let dto: PlaylistDto = parse(&response, "playlist")?;
        let own = dto.creator.as_ref().is_some_and(|c| c.id == Some(user));
        Ok((dto.into_summary(own), etag))
    }

    async fn artist_header(&self, id: u64) -> Result<ArtistRef> {
        let dto: ArtistDto = self
            .read(
                &format!("artists/{id}"),
                &[],
                Some(Subject::Artist(id)),
                "artist",
            )
            .await?;
        Ok(ArtistRef {
            id: dto.id,
            name: dto.name,
        })
    }

    // Lists -----------------------------------------------------------

    async fn list(
        &self,
        list: &ListRef,
        offset: u32,
        limit: u32,
        hidden_words: &[String],
    ) -> Result<ListItems> {
        Ok(match list {
            ListRef::FavoriteTracks => {
                ListItems::Tracks(self.favorite_tracks(offset, limit).await?)
            }
            ListRef::Playlists => ListItems::Playlists(self.playlists(offset, limit).await?),
            ListRef::FavoriteAlbums => {
                ListItems::Albums(self.favorite_albums(offset, limit).await?)
            }
            ListRef::FavoriteArtists => {
                ListItems::Artists(self.favorite_artists(offset, limit).await?)
            }
            ListRef::AlbumTracks(id) => {
                ListItems::Tracks(self.album_tracks(*id, offset, limit).await?)
            }
            ListRef::PlaylistTracks(uuid) => {
                ListItems::Tracks(self.playlist_tracks(uuid, offset, limit).await?)
            }
            ListRef::TopTracks(id) => ListItems::Tracks(self.top_tracks(*id, offset, limit).await?),
            ListRef::ArtistAlbums(id) => {
                ListItems::Albums(self.artist_albums(*id, offset, limit).await?)
            }
            ListRef::ArtistAppearsOn(id) => {
                ListItems::Albums(self.appears_on(*id, offset, limit).await?)
            }
            ListRef::Credits(id) => {
                ListItems::Credits(self.credits(*id, offset, limit, hidden_words).await?)
            }
            ListRef::SearchTracks(query) => {
                let page: RawPage<TrackDto> =
                    self.search_list("tracks", query, offset, limit).await?;
                ListItems::Tracks(page.map(stereo_track, offset))
            }
            ListRef::SearchAlbums(query) => {
                let page: RawPage<AlbumDto> =
                    self.search_list("albums", query, offset, limit).await?;
                ListItems::Albums(page.map(|a| Some(a.into()), offset))
            }
            ListRef::SearchArtists(query) => {
                let page: RawPage<ArtistDto> =
                    self.search_list("artists", query, offset, limit).await?;
                ListItems::Artists(page.map(|a| Some(ArtistRef::from(a)), offset))
            }
            ListRef::SearchPlaylists(query) => {
                let page: RawPage<PlaylistDto> =
                    self.search_list("playlists", query, offset, limit).await?;
                ListItems::Playlists(page.map(|p| Some(p.into_summary(false)), offset))
            }
        })
    }

    async fn favorite_tracks(&self, offset: u32, limit: u32) -> Result<ListPage<Track>> {
        let (user, _) = self.auth.account().await;
        let page: RawPage<Entry<TrackDto>> = self
            .page_of(
                &format!("users/{user}/favorites/tracks"),
                &[("order", "DATE"), ("orderDirection", "DESC")],
                offset,
                limit.clamp(1, LARGEST),
                None,
                "favorite tracks",
            )
            .await?;
        Ok(page.map(|e| Some(Track::from(e.item)), offset))
    }

    async fn playlists(&self, offset: u32, limit: u32) -> Result<ListPage<PlaylistSummary>> {
        let (user, _) = self.auth.account().await;
        let page: RawPage<PlaylistEntry> = self
            .page_of(
                &format!("users/{user}/playlistsAndFavoritePlaylists"),
                &[("order", "DATE"), ("orderDirection", "DESC")],
                offset,
                limit.clamp(1, LARGEST_SMALL),
                None,
                "playlists",
            )
            .await?;
        Ok(page.map(
            |e| {
                let own = e.kind.as_deref() == Some("USER_CREATED");
                Some(e.playlist.into_summary(own))
            },
            offset,
        ))
    }

    async fn favorite_albums(&self, offset: u32, limit: u32) -> Result<ListPage<AlbumSummary>> {
        let (user, _) = self.auth.account().await;
        let page: RawPage<Entry<AlbumDto>> = self
            .page_of(
                &format!("users/{user}/favorites/albums"),
                &[("order", "DATE"), ("orderDirection", "DESC")],
                offset,
                limit.clamp(1, LARGEST),
                None,
                "favorite albums",
            )
            .await?;
        Ok(page.map(|e| Some(e.item.into()), offset))
    }

    async fn favorite_artists(&self, offset: u32, limit: u32) -> Result<ListPage<ArtistRef>> {
        let (user, _) = self.auth.account().await;
        let page: RawPage<Entry<ArtistDto>> = self
            .page_of(
                &format!("users/{user}/favorites/artists"),
                &[("order", "DATE"), ("orderDirection", "DESC")],
                offset,
                limit.clamp(1, LARGEST),
                None,
                "favorite artists",
            )
            .await?;
        Ok(page.map(
            |e| {
                Some(ArtistRef {
                    id: e.item.id,
                    name: e.item.name,
                })
            },
            offset,
        ))
    }

    async fn album_tracks(&self, id: u64, offset: u32, limit: u32) -> Result<ListPage<Track>> {
        let page: RawPage<TrackDto> = self
            .page_of(
                &format!("albums/{id}/tracks"),
                &[],
                offset,
                limit.clamp(1, LARGEST),
                Some(Subject::Item(Item::Album(id))),
                "album",
            )
            .await?;
        Ok(page.map(|t| Some(Track::from(t)), offset))
    }

    /// Items that are not tracks (videos) are left out.
    async fn playlist_tracks(
        &self,
        uuid: &str,
        offset: u32,
        limit: u32,
    ) -> Result<ListPage<Track>> {
        let page: RawPage<TypedEntry> = self
            .page_of(
                &format!("playlists/{uuid}/items"),
                &[],
                offset,
                limit.clamp(1, LARGEST_ITEMS),
                Some(Subject::Item(Item::Playlist(uuid.to_owned()))),
                "playlist",
            )
            .await?;
        let mut malformed = false;
        let page = page.map(
            |e| {
                if e.kind != "track" {
                    return None;
                }
                match serde_json::from_value::<TrackDto>(e.item) {
                    Ok(dto) => Some(Track::from(dto)),
                    Err(_) => {
                        malformed = true;
                        None
                    }
                }
            },
            offset,
        );
        if malformed {
            return Err(LibraryError::Malformed("playlist"));
        }
        Ok(page)
    }

    async fn top_tracks(&self, id: u64, offset: u32, limit: u32) -> Result<ListPage<Track>> {
        let page: RawPage<TrackDto> = self
            .page_of(
                &format!("artists/{id}/toptracks"),
                &[],
                offset,
                limit.clamp(1, LARGEST),
                Some(Subject::Artist(id)),
                "top tracks",
            )
            .await?;
        Ok(page.map(|t| Some(Track::from(t)), offset))
    }

    async fn appears_on(&self, id: u64, offset: u32, limit: u32) -> Result<ListPage<AlbumSummary>> {
        let page: RawPage<AlbumDto> = self
            .page_of(
                &format!("artists/{id}/albums"),
                &[("filter", "COMPILATIONS")],
                offset,
                limit.clamp(1, LARGEST),
                Some(Subject::Artist(id)),
                "albums",
            )
            .await?;
        Ok(page.map(|a| Some(a.into()), offset))
    }

    /// The plain album list, then EPs and singles (see the module docs for
    /// how `offset` maps across the two).
    async fn artist_albums(
        &self,
        id: u64,
        offset: u32,
        limit: u32,
    ) -> Result<ListPage<AlbumSummary>> {
        let limit = limit.clamp(1, LARGEST);
        let path = format!("artists/{id}/albums");
        let subject = Some(Subject::Artist(id));
        let plain: RawPage<AlbumDto> = self
            .page_of(&path, &[], offset, limit, subject.clone(), "albums")
            .await?;
        let plain_total = plain.total;
        let slots = plain_total.div_ceil(u64::from(limit)) * u64::from(limit);
        let filter = [("filter", "EPSANDSINGLES")];
        let (items, eps_total) = if u64::from(offset) < slots {
            let eps: RawPage<AlbumDto> = self
                .page_of(&path, &filter, 0, 1, subject, "albums")
                .await?;
            (plain.items, eps.total)
        } else {
            let eps_offset = u32::try_from(u64::from(offset) - slots).unwrap_or(u32::MAX);
            let eps: RawPage<AlbumDto> = self
                .page_of(&path, &filter, eps_offset, limit, subject, "albums")
                .await?;
            (eps.items, eps.total)
        };
        Ok(ListPage {
            items: items.into_iter().map(Into::into).collect(),
            offset,
            total: clamp_total(plain_total + eps_total),
            hidden: 0,
        })
    }

    /// The artist's *All tracks* (see the module docs).
    async fn credits(
        &self,
        artist: u64,
        offset: u32,
        limit: u32,
        hidden_words: &[String],
    ) -> Result<ListPage<CreditedTrack>> {
        let limit = limit.clamp(1, LARGEST_SMALL);
        let cached = self.cached_credit_path(artist);
        let raw = if offset == 0 || cached.is_none() {
            let Some(list) = self.contributor(artist).await? else {
                return Ok(ListPage {
                    items: vec![],
                    offset,
                    total: 0,
                    hidden: 0,
                });
            };
            if offset == 0 {
                let mut list = list;
                list.items.truncate(limit as usize);
                list.into_page()
            } else {
                self.credit_data(artist, &list.data_api_path, offset, limit)
                    .await?
            }
        } else {
            let path = cached.unwrap_or_default();
            match self.credit_data(artist, &path, offset, limit).await {
                Err(LibraryError::NotFound(_)) => {
                    let Some(list) = self.contributor(artist).await? else {
                        return Ok(ListPage {
                            items: vec![],
                            offset,
                            total: 0,
                            hidden: 0,
                        });
                    };
                    self.credit_data(artist, &list.data_api_path, offset, limit)
                        .await?
                }
                other => other?,
            }
        };
        let (items, hidden) = credited(raw.items, hidden_words)?;
        Ok(ListPage {
            items,
            offset,
            total: clamp_total(raw.total),
            hidden,
        })
    }

    fn cached_credit_path(&self, artist: u64) -> Option<String> {
        self.credit_paths
            .lock()
            .expect("poisoned")
            .get(&artist)
            .cloned()
    }

    /// `GET /pages/contributor`: the `ITEM_LIST_WITH_ROLES` module's
    /// list, `None` when the page has no such module. Keeps its
    /// `dataApiPath`.
    async fn contributor(&self, artist: u64) -> Result<Option<PagedList>> {
        let id = artist.to_string();
        let page: ContributorPage = self
            .read(
                "pages/contributor",
                &[
                    ("artistId", id.as_str()),
                    ("deviceType", "BROWSER"),
                    ("locale", "en_US"),
                ],
                Some(Subject::Artist(artist)),
                "credits",
            )
            .await?;
        let list = page
            .rows
            .into_iter()
            .flat_map(|row| row.modules)
            .filter(|m| m.kind == "ITEM_LIST_WITH_ROLES")
            .find_map(|m| m.paged_list);
        if let Some(list) = &list {
            self.credit_paths
                .lock()
                .expect("poisoned")
                .insert(artist, list.data_api_path.clone());
        }
        Ok(list)
    }

    /// One page from a `dataApiPath`.
    async fn credit_data(
        &self,
        artist: u64,
        data_api_path: &str,
        offset: u32,
        limit: u32,
    ) -> Result<RawPage<CreditEntry>> {
        self.page_of(
            data_api_path,
            &[("deviceType", "BROWSER"), ("locale", "en_US")],
            offset,
            limit,
            Some(Subject::Artist(artist)),
            "credits",
        )
        .await
    }

    /// One page of a paged endpoint: `countryCode`, `extra`, `limit`,
    /// `offset`.
    async fn page_of<T: DeserializeOwned>(
        &self,
        path: &str,
        extra: &[(&str, &str)],
        offset: u32,
        limit: u32,
        subject: Option<Subject>,
        what: &'static str,
    ) -> Result<RawPage<T>> {
        let limit = limit.to_string();
        let offset = offset.to_string();
        let mut query = extra.to_vec();
        query.push(("limit", limit.as_str()));
        query.push(("offset", offset.as_str()));
        self.read(path, &query, subject, what).await
    }

    // Search ----------------------------------------------------------

    /// `GET /search`: the four lists' first pages and the top hit.
    async fn search(&self, query: &str, size: u32) -> Result<PageData> {
        let dto: SearchDto = self
            .search_read(
                "search",
                &[("query", query), ("types", SEARCH_TYPES)],
                0,
                size,
            )
            .await?;
        Ok(PageData::Search {
            top_hit: dto.top_hit.and_then(top_hit).map(Box::new),
            tracks: dto.tracks.map(stereo_track, 0),
            albums: dto.albums.map(|a| Some(a.into()), 0),
            artists: dto.artists.map(|a| Some(ArtistRef::from(a)), 0),
            playlists: dto.playlists.map(|p| Some(p.into_summary(false)), 0),
        })
    }

    /// `GET /search/{kind}`: one page of one list, bare items.
    async fn search_list<T: DeserializeOwned>(
        &self,
        kind: &str,
        query: &str,
        offset: u32,
        limit: u32,
    ) -> Result<RawPage<T>> {
        self.search_read(
            &format!("search/{kind}"),
            &[("query", query)],
            offset,
            limit,
        )
        .await
    }

    /// A search call: `extra`, `limit` (clamped) and `offset`; a `400` with
    /// a `userMessage` is [`LibraryError::SearchRefused`].
    async fn search_read<T: DeserializeOwned>(
        &self,
        path: &str,
        extra: &[(&str, &str)],
        offset: u32,
        limit: u32,
    ) -> Result<T> {
        let limit = limit.clamp(1, LARGEST).to_string();
        let offset = offset.to_string();
        let mut query = extra.to_vec();
        query.push(("limit", limit.as_str()));
        query.push(("offset", offset.as_str()));
        let response = self.fetch(path, &query).await?;
        if response.status().as_u16() == 400
            && let Some(message) = user_message(&response)
        {
            return Err(LibraryError::SearchRefused(message));
        }
        parse(&check(&response, None)?, "search")
    }

    // Favorites -------------------------------------------------------

    async fn is_favorite(&self, kind: FavoriteKind, id: &str) -> Result<bool> {
        let (user, _) = self.auth.account().await;
        let lists: HashMap<String, Vec<serde_json::Value>> = self
            .read(
                &format!("users/{user}/favorites/ids"),
                &[],
                None,
                "favorites",
            )
            .await?;
        let key = match kind {
            FavoriteKind::Track => "TRACK",
            FavoriteKind::Album => "ALBUM",
            FavoriteKind::Artist => "ARTIST",
            FavoriteKind::Playlist => "PLAYLIST",
        };
        Ok(lists.get(key).is_some_and(|ids| {
            ids.iter().any(|v| match v {
                serde_json::Value::String(s) => s == id,
                serde_json::Value::Number(n) => n.to_string() == id,
                _ => false,
            })
        }))
    }

    async fn add_favorite(&self, kind: FavoriteKind, id: &str) -> Result<()> {
        let (user, _) = self.auth.account().await;
        let (segment, field, subject) = favorite_target(kind, id)?;
        let response = self
            .write(
                Method::POST,
                &format!("users/{user}/favorites/{segment}"),
                Some(&[(field, id)]),
                &[],
            )
            .await?;
        check(&response, Some(subject)).map(drop)
    }

    async fn remove_favorite(&self, kind: FavoriteKind, id: &str) -> Result<()> {
        let (user, _) = self.auth.account().await;
        let (segment, _, subject) = favorite_target(kind, id)?;
        let response = self
            .write(
                Method::DELETE,
                &format!("users/{user}/favorites/{segment}/{id}"),
                None,
                &[],
            )
            .await?;
        check(&response, Some(subject)).map(drop)
    }

    // Playlists -------------------------------------------------------

    async fn create_playlist(&self, title: &str) -> Result<PlaylistSummary> {
        let (user, _) = self.auth.account().await;
        let response = self
            .write(
                Method::POST,
                &format!("users/{user}/playlists"),
                Some(&[("title", title), ("description", "")]),
                &[],
            )
            .await?;
        let dto: PlaylistDto = parse(&check(&response, None)?, "playlist")?;
        Ok(dto.into_summary(true))
    }

    /// The playlist's current `ETag`.
    async fn playlist_etag(&self, uuid: &str) -> Result<String> {
        let response = self
            .get(
                &format!("playlists/{uuid}"),
                &[],
                Some(Subject::Item(Item::Playlist(uuid.to_owned()))),
            )
            .await?;
        response
            .header("etag")
            .map(str::to_owned)
            .ok_or(LibraryError::Malformed("playlist"))
    }

    async fn add_to_playlist(
        &self,
        uuid: &str,
        tracks: &[TrackId],
        allow_duplicates: bool,
    ) -> Result<()> {
        if tracks.is_empty() {
            return Ok(());
        }
        let subject = Subject::Item(Item::Playlist(uuid.to_owned()));
        let path = format!("playlists/{uuid}/items");
        let mut etag = self.playlist_etag(uuid).await?;
        for chunk in tracks.chunks(ADD_CHUNK) {
            let ids = chunk
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(",");
            let mut form = vec![("trackIds", ids.as_str())];
            if allow_duplicates {
                form.push(("onDupes", "ADD"));
            }
            let mut retried = false;
            loop {
                let response = self
                    .write(
                        Method::POST,
                        &path,
                        Some(&form),
                        &[("if-none-match", etag.as_str())],
                    )
                    .await?;
                if response.status().as_u16() == 412 {
                    if retried {
                        return Err(LibraryError::PlaylistChanged);
                    }
                    retried = true;
                    etag = self.playlist_etag(uuid).await?;
                    continue;
                }
                check(&response, Some(subject.clone()))?;
                etag = match response.header("etag") {
                    Some(next) => next.to_owned(),
                    None => self.playlist_etag(uuid).await?,
                };
                break;
            }
        }
        Ok(())
    }

    /// Never retried: a `412` means positions may have moved.
    async fn remove_from_playlist(&self, uuid: &str, index: u32, etag: &str) -> Result<()> {
        let response = self
            .write(
                Method::DELETE,
                &format!("playlists/{uuid}/items/{index}"),
                None,
                &[("if-none-match", etag)],
            )
            .await?;
        check_edit(&response, uuid).map(drop)
    }

    async fn delete_playlist(&self, uuid: &str) -> Result<()> {
        let etag = self.playlist_etag(uuid).await?;
        let response = self
            .write(
                Method::DELETE,
                &format!("playlists/{uuid}"),
                None,
                &[("if-none-match", etag.as_str())],
            )
            .await?;
        check_edit(&response, uuid).map(drop)
    }

    // HTTP ------------------------------------------------------------

    /// `GET` with `countryCode`; a `404`/`2001` is `subject`'s not-found.
    async fn get(
        &self,
        path: &str,
        extra: &[(&str, &str)],
        subject: Option<Subject>,
    ) -> Result<ApiResponse> {
        check(&self.fetch(path, extra).await?, subject)
    }

    /// `GET` with `countryCode`; the response whatever its status.
    async fn fetch(&self, path: &str, extra: &[(&str, &str)]) -> Result<ApiResponse> {
        let (_, country) = self.auth.account().await;
        let mut query = vec![("countryCode", country.as_str())];
        query.extend_from_slice(extra);
        Ok(self.auth.get(path, &query, Some(REQUEST_TIMEOUT)).await?)
    }

    /// [`Self::get`], and the body decoded.
    async fn read<T: DeserializeOwned>(
        &self,
        path: &str,
        extra: &[(&str, &str)],
        subject: Option<Subject>,
        what: &'static str,
    ) -> Result<T> {
        parse(&self.get(path, extra, subject).await?, what)
    }

    /// A write with `countryCode`; the response is returned whatever its
    /// status.
    async fn write(
        &self,
        method: Method,
        path: &str,
        form: Option<&[(&str, &str)]>,
        headers: &[(&str, &str)],
    ) -> Result<ApiResponse> {
        let (_, country) = self.auth.account().await;
        let query = [("countryCode", country.as_str())];
        Ok(self
            .auth
            .send(method, path, &query, form, headers, Some(REQUEST_TIMEOUT))
            .await?)
    }
}

/// The path segment, form field and not-found subject of a favorite.
fn favorite_target(kind: FavoriteKind, id: &str) -> Result<(&'static str, &'static str, Subject)> {
    let number = || {
        id.parse::<u64>()
            .map_err(|_| LibraryError::InvalidId(id.to_owned()))
    };
    Ok(match kind {
        FavoriteKind::Track => (
            "tracks",
            "trackIds",
            Subject::Item(Item::Track(TrackId(number()?))),
        ),
        FavoriteKind::Album => ("albums", "albumIds", Subject::Item(Item::Album(number()?))),
        FavoriteKind::Artist => ("artists", "artistIds", Subject::Artist(number()?)),
        FavoriteKind::Playlist => (
            "playlists",
            "uuids",
            Subject::Item(Item::Playlist(id.to_owned())),
        ),
    })
}

/// A success is returned as is; a `404`/`2001` is `subject`'s not-found; any
/// other status is an [`AuthError::Http`].
fn check(response: &ApiResponse, subject: Option<Subject>) -> Result<ApiResponse> {
    let status = response.status();
    if status.is_success() {
        return Ok(response.clone());
    }
    if status.as_u16() == 404
        && response.sub_status() == Some(SUB_STATUS_NOT_FOUND)
        && let Some(subject) = subject
    {
        return Err(LibraryError::NotFound(subject));
    }
    Err(AuthError::Http {
        status: status.as_u16(),
        code: None,
    }
    .into())
}

/// [`check`] for an edit of playlist `uuid`: `412` is the changed message.
fn check_edit(response: &ApiResponse, uuid: &str) -> Result<ApiResponse> {
    if response.status().as_u16() == 412 {
        return Err(LibraryError::PlaylistChanged);
    }
    check(
        response,
        Some(Subject::Item(Item::Playlist(uuid.to_owned()))),
    )
}

fn parse<T: DeserializeOwned>(response: &ApiResponse, what: &'static str) -> Result<T> {
    response.json().map_err(|_| LibraryError::Malformed(what))
}

/// Tidal's `userMessage` from an error body, when it has a non-empty one.
fn user_message(response: &ApiResponse) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        #[serde(rename = "userMessage")]
        user_message: Option<String>,
    }
    let message = response.json::<ErrorBody>().ok()?.user_message?;
    (!message.trim().is_empty()).then_some(message)
}

/// The track, unless it has no stereo mode (Dolby-Atmos-only).
fn stereo_track(dto: TrackDto) -> Option<Track> {
    dto.audio_modes
        .iter()
        .any(|m| m == "STEREO")
        .then(|| Track::from(dto))
}

/// A search's `topHit` (`{type, value}`) by its `type`; any other type, or
/// a `value` that cannot be read, is none.
fn top_hit(hit: serde_json::Value) -> Option<TopHit> {
    let kind = hit.get("type")?.as_str()?.to_owned();
    let value = hit.get("value")?.clone();
    match kind.as_str() {
        "TRACKS" => stereo_track(serde_json::from_value(value).ok()?).map(TopHit::Track),
        "ALBUMS" => Some(TopHit::Album(
            serde_json::from_value::<AlbumDto>(value).ok()?.into(),
        )),
        "ARTISTS" => Some(TopHit::Artist(
            serde_json::from_value::<ArtistDto>(value).ok()?.into(),
        )),
        "PLAYLISTS" => Some(TopHit::Playlist(
            serde_json::from_value::<PlaylistDto>(value)
                .ok()?
                .into_summary(false),
        )),
        _ => None,
    }
}

fn clamp_total(total: u64) -> u32 {
    u32::try_from(total).unwrap_or(u32::MAX)
}

/// The credits' rows with their role categories, minus the Atmos-only
/// copies and the alternate versions; and how many were left out.
fn credited(entries: Vec<CreditEntry>, words: &[String]) -> Result<(Vec<CreditedTrack>, u32)> {
    let mut items = Vec::new();
    let mut hidden = 0;
    for entry in entries {
        if entry.kind.as_deref().is_some_and(|k| k != "track") {
            continue;
        }
        let dto: TrackDto =
            serde_json::from_value(entry.item).map_err(|_| LibraryError::Malformed("credits"))?;
        let stereo = dto.audio_modes.iter().any(|m| m == "STEREO");
        let track = Track::from(dto);
        if !stereo || hidden_version(&track, words) {
            hidden += 1;
            continue;
        }
        let mut roles: Vec<RoleCategory> = Vec::new();
        for role in entry.roles {
            if let Some(category) = role_category(role.category_id)
                && !roles.contains(&category)
            {
                roles.push(category);
            }
        }
        items.push(CreditedTrack { track, roles });
    }
    Ok((items, hidden))
}

/// Tidal's `categoryId` of a role category.
fn role_category(id: i64) -> Option<RoleCategory> {
    match id {
        11 => Some(RoleCategory::Performer),
        2 => Some(RoleCategory::Songwriter),
        1 => Some(RoleCategory::Producer),
        3 => Some(RoleCategory::Engineer),
        _ => None,
    }
}

// DTOs ------------------------------------------------------------------

/// A paged response: the items are the endpoint's own envelope.
#[derive(Deserialize)]
struct RawPage<T> {
    #[serde(default, rename = "totalNumberOfItems")]
    total: u64,
    items: Vec<T>,
}

impl<T> RawPage<T> {
    /// Maps each item (`None` drops it) into a core page asked at `offset`.
    fn map<U>(self, mut f: impl FnMut(T) -> Option<U>, offset: u32) -> ListPage<U> {
        ListPage {
            items: self.items.into_iter().filter_map(&mut f).collect(),
            offset,
            total: clamp_total(self.total),
            hidden: 0,
        }
    }
}

/// `GET /search`: the four lists asked for (`videos` ignored) and the top
/// hit, read by [`top_hit`].
#[derive(Deserialize)]
struct SearchDto {
    tracks: RawPage<TrackDto>,
    albums: RawPage<AlbumDto>,
    artists: RawPage<ArtistDto>,
    playlists: RawPage<PlaylistDto>,
    #[serde(default, rename = "topHit")]
    top_hit: Option<serde_json::Value>,
}

/// `{created, item}`.
#[derive(Deserialize)]
struct Entry<T> {
    item: T,
}

/// `{type, item}` of a playlist's items.
#[derive(Deserialize)]
struct TypedEntry {
    #[serde(rename = "type")]
    kind: String,
    item: serde_json::Value,
}

/// `{type: USER_CREATED | USER_FAVORITE, playlist}`.
#[derive(Deserialize)]
struct PlaylistEntry {
    #[serde(rename = "type")]
    kind: Option<String>,
    playlist: PlaylistDto,
}

#[derive(Deserialize)]
struct Creator {
    id: Option<u64>,
}

#[derive(Deserialize)]
struct PlaylistDto {
    uuid: String,
    title: String,
    #[serde(rename = "numberOfTracks")]
    tracks: Option<u32>,
    /// Whole seconds.
    duration: Option<u64>,
    creator: Option<Creator>,
}

impl PlaylistDto {
    fn into_summary(self, own: bool) -> PlaylistSummary {
        PlaylistSummary {
            uuid: self.uuid,
            title: self.title,
            tracks: self.tracks,
            duration: self.duration.map(Duration::from_secs),
            own,
        }
    }
}

#[derive(Deserialize)]
struct AlbumDto {
    id: u64,
    title: String,
    #[serde(default)]
    artists: Vec<ArtistDto>,
    artist: Option<ArtistDto>,
    /// `YYYY-MM-DD`.
    #[serde(rename = "releaseDate")]
    release_date: Option<String>,
    /// `ALBUM`, `EP`, `SINGLE`, ...
    #[serde(rename = "type")]
    kind: Option<String>,
    #[serde(rename = "numberOfTracks")]
    tracks: Option<u32>,
    /// Whole seconds.
    duration: Option<u64>,
}

impl From<AlbumDto> for AlbumSummary {
    fn from(dto: AlbumDto) -> Self {
        let artists: Vec<ArtistDto> = if dto.artists.is_empty() {
            dto.artist.into_iter().collect()
        } else {
            dto.artists
        };
        AlbumSummary {
            id: dto.id,
            title: dto.title,
            artists: artists
                .into_iter()
                .map(|a| ArtistRef {
                    id: a.id,
                    name: a.name,
                })
                .collect(),
            year: dto
                .release_date
                .as_deref()
                .and_then(|d| d.get(..4))
                .and_then(|y| y.parse().ok()),
            kind: match dto.kind.as_deref() {
                Some("EP") => AlbumKind::Ep,
                Some("SINGLE") => AlbumKind::Single,
                _ => AlbumKind::Album,
            },
            tracks: dto.tracks,
            duration: dto.duration.map(Duration::from_secs),
        }
    }
}

#[derive(Deserialize)]
struct ContributorPage {
    #[serde(default)]
    rows: Vec<ContributorRow>,
}

#[derive(Deserialize)]
struct ContributorRow {
    #[serde(default)]
    modules: Vec<ContributorModule>,
}

#[derive(Deserialize)]
struct ContributorModule {
    #[serde(default, rename = "type")]
    kind: String,
    #[serde(rename = "pagedList")]
    paged_list: Option<PagedList>,
}

#[derive(Deserialize)]
struct PagedList {
    #[serde(rename = "dataApiPath")]
    data_api_path: String,
    #[serde(default, rename = "totalNumberOfItems")]
    total: u64,
    #[serde(default)]
    items: Vec<CreditEntry>,
}

impl PagedList {
    fn into_page(self) -> RawPage<CreditEntry> {
        RawPage {
            total: self.total,
            items: self.items,
        }
    }
}

#[derive(Deserialize)]
struct CreditEntry {
    #[serde(default, rename = "type")]
    kind: Option<String>,
    item: serde_json::Value,
    #[serde(default)]
    roles: Vec<RoleDto>,
}

#[derive(Deserialize)]
struct RoleDto {
    #[serde(rename = "categoryId")]
    category_id: i64,
}
