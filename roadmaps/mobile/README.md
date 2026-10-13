# Android and touch maturity

Parent M7.2/M2.4/M6.4; ROADMAP_NEW Q; expansion §6; 1010 V1–V4/V11.
[Platform guide](../../android/README.md).

| ID | Status | Remaining work / acceptance |
| --- | --- | --- |
| M1 | Certification-only | Physical phone/tablet: pan/pinch/tilt/orbit/scrub, pen/keyboard/IME/Back, touch targets and tool/map ownership. Record portrait/landscape and split-screen across the 600 dp workstation boundary. |
| M2 | Certification-only | Pause/resume, surface/GPU recreation, offline/reconnect, notification service timeout and warning geometry on actual Android versions. Build/JVM geometry tests cannot close these. |
| M3 | Partially implemented | Memory-pressure response exists; validate bounded caches/workers and recovery under constrained-device load, then fix demonstrated gaps. Include 16 KB page-device compatibility and release-library alignment. |
| M4 | Partially implemented | Remove remaining hover-only/small workstation targets and non-colour-only ambiguity exposed by device runs. Preserve phone chrome and existing workstation tablet decision. |

The older “missing onTimeout”, “six rim-point warning sampling” and “no trim callback”
requests are superseded by current Kotlin/Rust work. Do not rebuild those handlers.
No physical device was established by this audit. The installed Android Rust target
alone is not an SDK, APK build, device or lifecycle result. Track C remains open.
