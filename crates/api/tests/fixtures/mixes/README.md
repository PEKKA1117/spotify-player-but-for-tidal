# Fixtures for spec 0011 (mixes and radio)

Written from the shapes the live probe of 2026-10-09 found (see the spec's
"Facts"): catalogue data is invented, user IDs replaced, image URLs and
tokens dropped. Mix IDs are 30 lower-case hex digits.

- `mixes_page.json`: `/pages/my_collection_my_mixes`, one module holding its total (6): a discovery mix, a daily mix, a video mix, a repeated ID, an empty ID and a daily mix with an empty `subTitle`
- `mixes_page_first.json`, `mixes_data_2.json`, `mixes_data_4.json`: a module holding 2 of 5 mixes and the `dataApiPath` answers for `offset=2` and `offset=4` (each says `offset: 0`)
- `mixes_page_empty.json`: no mixes
- `pages_mix.json`: `/pages/mix`, the `MIX_HEADER` module
- `mix_items.json`: `/mixes/{id}/items`, five entries: two tracks, a `video`, an Atmos-only track and a track
- `track_header.json`, `track_radio.json`, `artist_header.json`, `artist_radio.json`: the radio calls and their headers; the Atmos-only track is to be dropped
- `error_*.json`: `404`/`2001` bodies
