# Built-in sounds

Every built-in sound is **procedurally generated** by `tools/gen-builtin-sounds` (fixed seed, no recordings or samples from anyone else) and dedicated to the public domain under [CC0-1.0](https://creativecommons.org/publicdomain/zero/1.0/).

Regenerate with `cargo run --release -p gen-builtin-sounds`; the output is identical each time.

| File | 名稱 | Name | How it is made |
|------|------|------|----------------|
| `glass-01.wav` | 玻璃碎裂 01 | Glass shatter 01 | noise burst + 40 decaying high partials |
| `glass-02.wav` | 玻璃碎裂 02 | Glass shatter 02 | noise burst + 70 shards spread over ~1 s |
| `glass-crash.wav` | 玻璃大爆炸 | Glass avalanche | three overlapping shatters |
| `metal-snap.wav` | 金屬斷裂 | Metal snap | click + inharmonic bar modes (310 Hz) |
| `metal-bend.wav` | 鋼筋彎折 | Rebar bending | slow stick-slip groan through metal resonances, ending in a ping |
| `thud-low.wav` | 悶響 · 低 | Low thud | falling 90 → 45 Hz sine + low-passed noise |
| `creak.wav` | 吱嘎門軸 | Door creak | stick-slip friction with a sweeping rate through wood resonances |
| `oh.wav` | 喔！ | Oh! | formant voice, vowel 'o', falling pitch |
| `hmm.wav` | 嗯？ | Hm? | formant voice, nasal 'm', rising pitch |
| `aiyo.wav` | 哎唷 | Ouch | formant voice, 'a' → 'i' → 'o' with a pitch drop |
| `whimper.wav` | 嗚… | Whimper | formant voice, vowel 'u', slow falling pitch with wobble |
| `scream.wav` | 啊——！ | Aaah! | formant voice, vowel 'a', high rising then falling pitch |
| `yelps.wav` | 哇哇哇 | Yelp yelp yelp | three quick formant yelps, each higher |

| Profile | Pack |
|---------|------|
| 毀滅（Destruction） | `builtin/destruction.spankpack` |
| 哀哀叫（Whiner） | `builtin/whine.spankpack` |
| 經典（Classic） | `builtin/classic.spankpack` |
