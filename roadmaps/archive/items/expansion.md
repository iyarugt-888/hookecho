# expansion.md — migration dispositions

[Original bytes](../sources/expansion.md) · [current plan](../../README.md)

Rows name original line numbers. Section rows include their subordinate prose/list
requirements. Evidence is historical; statuses do not claim fresh tests.

| Line | Item | Status | Destination / evidence |
| --- | --- | --- | --- |
| 1 | Expansion — desktop and Android, off the web build's budget | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 6 | Why this document exists | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 32 | 1. Where things stand, measured | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 72 | 2. The actual ceilings | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 74 | 2.1 Web — the wall is not the one being gated | partially implemented | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 105 | 2.2 Android | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 135 | 2.3 Windows | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 152 | 2.4 Runtime ceilings, which bind long before file size | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 171 | 3. Proposed budgets | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 194 | 4. Keeping it separate | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 199 | 4.1 Gate the raw wasm size — `scripts/web/build.sh` — **done** | partially implemented | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 209 | 4.2 Gate the web dependency graph — new step in `ci.yml` | partially implemented | [radar-analysis](../../radar-analysis/README.md); [evidence](../../../docs/certification/m3.3/README.md) |
| 234 | 4.3 Make native-only the default for new code — `crates/hookecho/src/native/` | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 257 | 4.4 What already works and should not be touched | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 270 | 5. What to build on native | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 276 | 5.1 No filesystem — storage-unbounded work | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 291 | 5.2 No process spawning — the extension path | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 301 | 5.3 No sockets — the local-network product | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 311 | 5.4 No threads — CPU-heavy analysis | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 330 | 5.5 Android-specific | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 341 | 5.6 Desktop-specific | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 348 | 6. Fix before expanding Android | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 353 | 6.1 16 KB page alignment — NDK r26d is too old | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 378 | 6.2 Android 15 foreground-service timeout — a live crash | partially implemented | [mobile](../../mobile/README.md); [evidence](../../../android/README.md) |
| 411 | 6.3 Memory pressure — nothing listens | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 426 | 6.4 The APK is arm64-v8a only | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 435 | 7. The no-third-party-approval constraint | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 440 | 7.1 Data sources | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 467 | 7.2 Distribution channels | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 493 | 8. Sequencing | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 530 | 9. How to tell whether this worked | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |
| 546 | Sources | partially implemented | [certification](../../certification/README.md); [evidence](../../../docs/certification/README.md) |

Classification bases (by family):

- **Expansion — desktop and Android, off the web build's budget**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Why this document exists**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **1. Where things stand, measured**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2. The actual ceilings**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.1 Web — the wall is not the one being gated**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.2 Android**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.3 Windows**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **2.4 Runtime ceilings, which bind long before file size**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **3. Proposed budgets**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4. Keeping it separate**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.1 Gate the raw wasm size — `scripts/web/build.sh` — **done****: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.2 Gate the web dependency graph — new step in `ci.yml`**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.3 Make native-only the default for new code — `crates/hookecho/src/native/`**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **4.4 What already works and should not be touched**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5. What to build on native**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.1 No filesystem — storage-unbounded work**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.2 No process spawning — the extension path**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.3 No sockets — the local-network product**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.4 No threads — CPU-heavy analysis**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.5 Android-specific**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **5.6 Desktop-specific**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6. Fix before expanding Android**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.1 16 KB page alignment — NDK r26d is too old**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.2 Android 15 foreground-service timeout — a live crash**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.3 Memory pressure — nothing listens**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **6.4 The APK is arm64-v8a only**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7. The no-third-party-approval constraint**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7.1 Data sources**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **7.2 Distribution channels**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **8. Sequencing**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **9. How to tell whether this worked**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
- **Sources**: Existing foundations; retain the complete source acceptance scope under the linked workstream. Mixed or unverified requirements remain open.
