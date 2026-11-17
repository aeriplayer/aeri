# aeri-player fixtures

Hand-written M3U text for `tests/m3u.rs`, and the seed corpus of the M3U fuzz target (`fuzz/`).

```bash
printf 'one.flac\nalbum/two.mp3\n/absolute/three.flac\n' > basic.m3u

printf '#EXTM3U\n#EXTINF:123,Artist - First\none.flac\n#EXTINF:-1,Artist - Second\nalbum/two.mp3\n' > extended.m3u

printf '# a messy playlist\n\nindented.flac\n\nhttp://example.com/stream.mp3\nfile:///not/played.flac\n#EXTINF:1,x\nwith space/and #hash.flac\n' > messy.m3u

printf '#EXTM3U\n# nothing here\n' > empty.m3u

printf 'café.flac\n東京/two.mp3\n' > unicode.m3u
```

- `basic.m3u`: two relative paths and one absolute.
- `extended.m3u`: the `#EXTM3U` header and `#EXTINF` lines, which are comments to aeri.
- `messy.m3u`: a blank line, comments, two URLs, and a `#` inside a name.
- `empty.m3u`: a header and a comment, no entries.
- `unicode.m3u`: non-ASCII names.

There is no fixture with a byte-order mark, CRLF line endings, or whitespace around a path. The repository's whitespace and line-ending hooks rewrite exactly those bytes in a text file, which would leave the fixture testing something else. Those cases are covered by the parser's unit tests, which build the text in code.
