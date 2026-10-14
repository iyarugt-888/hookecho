# Completed TGFTP fallback fixture

`KFTG_20261013_002258.bz2` is the unchanged public NOAA TGFTP object downloaded
2026-10-13 UTC from
https://tgftp.nws.noaa.gov/data/radar/nexrad_level2/KFTG/KFTG_20261013_002258.bz2.

- Source: NOAA/NWS NEXRAD Level II, KFTG, nominal scan 2026-10-13 00:22:58 UTC.
- Size: 3,379,980 bytes.
- SHA-256: `281c815fec8658af5f6a49df12ceb0112021960b31ddfffa3dd30853c266cb7c`.
- Format: Archive II header and individually compressed records, despite the `.bz2`
  suffix. This is public US government radar data; no third-party account required.
- Purpose: completed-provider HTTP/cache/identity and end-of-scan regression. Tests
  require an actual decoded `ScanEnd` and KFTG site, preserving gates/source clocks.
  This is not a tornado/detection truth fixture or proof of complete spatial sampling.

The existing partial Denver hail fixture in `wxdata/tests/data/corpus/` remains a
negative control: it decodes useful gates but has no end-of-scan marker, and must
not be admitted as a completed fallback. Its original source/hash manifest is
unchanged. The fault test serves that partial payload under a newer test filename
deliberately; this is injected faulty transport metadata, not a claim about its date.

Discovery: the real KTLX TGFTP listing included a 2,343-byte newest object while it
was still being published. The previous adapter selected that object alone and
failed with “volume has no sweeps”; a larger prefix can decode and silently look
complete unless the source end marker is checked. Local tests cover both refusal
and recovery to the preceding completed object, bounded to three candidates.
