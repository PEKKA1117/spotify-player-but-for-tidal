//! Browse pages and the personal home feed (spec 0024). A child of
//! `library` (see its `mod browse`), so it reads the client's HTTP helpers
//! and DTOs without widening their visibility.

use tidal_player_core::library::{BrowseRef, ListItems, PageData};

use super::{LibraryClient, LibraryError, Result};

/// Which kind of path [`browse_path`] accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    /// A v1 `pages/…` path.
    V1,
    /// A v2 `home/pages/…` path.
    V2,
}

/// Stub until the green step.
fn browse_path(path: &str, _source: Source) -> Result<&str> {
    Ok(path)
}

impl LibraryClient {
    /// Stub until the green step.
    pub(super) async fn browse_page(&self, _target: &BrowseRef) -> Result<PageData> {
        Err(LibraryError::Malformed("browse"))
    }

    /// Stub until the green step.
    pub(super) async fn browse_data(
        &self,
        _data: &str,
        _offset: u32,
        _limit: u32,
    ) -> Result<ListItems> {
        Err(LibraryError::Malformed("browse"))
    }

    /// Stub until the green step.
    pub(super) async fn browse_view_all(&self, _view_all: &str) -> Result<ListItems> {
        Err(LibraryError::Malformed("browse"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AC8: a path is followed only if it starts with `pages/` (v1) or
    /// `home/pages/` (v2) and has no `..` segment, plain or percent-encoded.
    #[test]
    fn ac8_browse_paths() {
        use Source::{V1, V2};
        let table = [
            (V1, "pages/home", true),
            (
                V1,
                "pages/data/00000000-0000-4000-8000-0000000000aa?albumId=10581228",
                true,
            ),
            (V1, "pages/single-module-page/f7bc/7/02f1/1", true),
            (V1, "pages/a..b", true),
            (V1, "pages/x?back=../", true),
            (V1, "home/pages/DAILY_MIXES/view-all", false),
            (V1, "pages", false),
            (V1, "pages/", false),
            (V1, "", false),
            (V1, "/pages/home", false),
            (V1, "pagesx/home", false),
            (V1, "Pages/home", false),
            (V1, "pages/../tracks/1", false),
            (V1, "pages/./home", false),
            (V1, "pages/a/..", false),
            (V1, "pages/%2e%2e/tracks/1", false),
            (V1, "pages/%2E./tracks/1", false),
            (V1, "pages\\..\\x", false),
            (V1, "https://example.com/pages/home", false),
            (V2, "home/pages/DAILY_MIXES/view-all", true),
            (
                V2,
                "home/pages/X/view-all?itemId=heavy%20alternative&rankingToken=1",
                true,
            ),
            (V2, "pages/home", false),
            (V2, "home/feed/static", false),
            (V2, "home/pages/../feed/static", false),
            (V2, "home/pages/%2e%2e/x", false),
            (V2, "", false),
        ];
        for (source, path, ok) in table {
            let got = browse_path(path, source);
            if ok {
                assert_eq!(got, Ok(path), "{source:?} {path}");
            } else {
                assert_eq!(
                    got,
                    Err(LibraryError::NotAPage(path.to_owned())),
                    "{source:?} {path}"
                );
            }
        }
    }
}
