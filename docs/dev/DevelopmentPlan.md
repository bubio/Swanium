# Swanium 開発計画書

> [Blueprint.md](./Blueprint.md) で定義された技術スタック・アーキテクチャ方針を前提に、
> 「実際にどの順序で何を実装していくか」を定める開発ロードマップ。

------------------------------------------------------------------------

# 1. 設計方針サマリ

-   **Cycle-accurate-first**: 命令単位（instruction-step）の粗い実行ではなく、
    クロック（t-state）単位でCPU・PPU・APU・タイマー・DMAを同期駆動する設計を
    最初から採用する。後から精度を上げる「とりあえず動かす」アプローチは取らない。
-   **モノクロ優先、Color拡張は抽象化で先取り**: PPU/カートリッジ/RTCは
    トレイトと機種フラグ（`HardwareModel::Mono | Color | Crystal`）で抽象化し、
    Phase 8でColor固有実装を追加するだけで済む構造にする。
-   **coreはプラットフォーム非依存**: `crates/core`はSlint/wgpu/cpal/gilrsに
    一切依存しない。フレームバッファ・オーディオサンプル・入力状態はプレーンな
    データ構造でcoreの境界をまたぐ。
-   **RetroAchievements (RA) 対応可能性を初期設計から確保**: 詳細は
    [7. RetroAchievements対応を見据えた設計](#7-retroachievements-ra-対応を見据えた設計)
    を参照。決定論的実行・安定したメモリ参照APIを要件として設計に織り込む。
-   **テストROM駆動**: Phase 3で公開CPUテストROMをパスすることを
    マイルストーンの「証明可能なDoD」とする。

------------------------------------------------------------------------

# 2. 参考資料・関連リポジトリ

開発にあたり、以下のローカル参考実装と公開資料を参照する。

## ローカル参考実装

-   `~/dev/_Emu/Original/WonderCrab` — Rust製のWonderSwanエミュレータ
    （exerciseプロジェクト）。精度・パフォーマンスは限定的だが、以下のモジュール構成は
    Swaniumのサブシステム分割の参考になる:
    -   `src/bus`（io_bus: eeprom/keypad, mem_bus）
    -   `src/cartridge`（cart_ports）
    -   `src/cpu/v30mz`（alu_ops, bit_ops, block_ops, ctrl_ops, mem_ops, util）
    -   `src/display`（display_control, screen, sprite）
    -   `src/dma`（gdma, sdma）— **DMA(GDMA/SDMA)サブシステム**の参考実装
    -   `src/sound`（channel）
    -   `src/soc` — 上記サブシステムを統合するSoCトップレベル

## 公開技術資料・テストROM・参考エミュレータ

WonderCrabのreadme.mdに掲載されている資料一覧（出典: WonderCrab readme.md）:

-   [WSDev Wiki](https://ws.nesdev.org/wiki/WSdev_Wiki)
-   [WonderSwan - Sacred Tech Scroll](http://perfectkiosk.net/stsws.html)
-   [WonderSwan CPU test (FluBBaOfWard/WSCPUTest)](https://github.com/FluBBaOfWard/WSCPUTest)
-   [WonderSwan test suite (asiekierka/ws-test-suite)](https://github.com/asiekierka/ws-test-suite)
-   [Mesen](https://www.mesen.ca/)
-   [Ares](https://ares-emu.net/)

これらはPhase 3（CPUテストROM検証）・PPU/APU実装時の主要リファレンスとし、
挙動が不明確な箇所はMesen/Ares/WonderCrabの実装との比較によるクロスチェックに用いる。

------------------------------------------------------------------------

# 3. Crate依存関係とビルド順序

```
common  (依存なし。utils, logging, config, error types)
  ^
  |
core    (依存: common のみ。CPU/Memory/Interrupt/Timer/DMA/PPU/APU/Cartridge)
  ^
  |--------------------+--------------------+
  |                    |                    |
video               audio                input
(依存: core, wgpu)  (依存: core, cpal)   (依存: core, gilrs)
  ^                    ^                    ^
  +--------------------+--------------------+
                       |
                  frontend
            (依存: core, video, audio, input, common, slint)
```

-   **ビルド順序**: `common` → `core` → (`video`, `audio`, `input` は互いに独立、並列可) → `frontend`
-   **core単体でビルド・テスト可能**であることをCIで強制する（`core`のCargo.tomlに
    `slint`/`wgpu`/`cpal`/`gilrs`への依存が一切無いことをlintまたはCIスクリプトで検証）。
-   `video`/`audio`/`input`は本来薄いアダプタ層なので、Phase 7以前は最小実装
    （フレームバッファをRGBAテクスチャに変換する程度）でよい。

------------------------------------------------------------------------

# 4. フェーズ別ロードマップ

## Phase 0 — ワークスペース基盤構築

**目的/DoD**: `cargo build` / `cargo test` / `cargo clippy` がワークスペース全体で
成功し、CIが緑になる状態。

**主要タスク**
-   ルート`Cargo.toml`（workspace, resolver="2"）、`rust-toolchain.toml`（stable固定、MSRV決定）作成
-   `crates/common`: エラー型、ロギング、基本設定構造体の骨格のみ
-   `crates/core`: 空のlib crateとして作成、依存はcommonのみ
-   `crates/video`, `crates/audio`, `crates/input`, `crates/frontend`: 空のスタブcrate
-   CI設定（fmt --check, clippy -D warnings, test をmacOS/Linux/Windowsで実行）
-   `tests/`ディレクトリの骨格（[8. テスト戦略詳細](#8-テスト戦略詳細)参照）

**依存関係**: なし（最初のフェーズ）

**テスト方法**: CIのビルド成功自体がDoD。

---

## Phase 1 — CPU実装（NEC V30MZ コア、メモリ抜きの命令セット）

**目的/DoD**: V30MZの主要命令セットがデコード・実行でき、レジスタ/フラグの状態を
単体で検証できる。この段階ではメモリは最小スタブ（フラットなバイト配列）でよい。

**主要タスク（`crates/core` 内 `cpu`モジュール）**
-   レジスタセット定義: AX/BX/CX/DX/SP/BP/SI/DI、セグメントレジスタ（CS/DS/SS/ES）、IP、フラグレジスタ
-   命令デコーダ: opcodeテーブル方式。8086/V30互換命令 + V30MZ固有の差異
    （BCD命令の挙動差、未定義命令の扱い）を明示的にコメント・テストで記録
-   アドレッシングモード解決（ModRM相当、20-bitセグメント:オフセット計算）
-   実行ユニット: 算術/論理/転送/分岐/スタック操作/文字列命令/REPプレフィックス
-   **tickベース実行モデルの骨格**: 命令を「サイクルカウント付き命令」として表現し、
    `step_cycle()`で1クロックずつ状態を進められるAPIを用意する
    （詳細は[5. サイクル精度設計の考慮点](#5-サイクル精度設計の考慮点)）
-   メモリアクセスは`MemoryBus`トレイト経由（Phase 1ではテスト用フラットメモリ実装）

**依存関係**: Phase 0完了後。

**テスト方法**
-   命令ごとのユニットテスト（`crates/core/src/cpu/tests/`）: 代表的なオペランド組み合わせごとに
    レジスタ・フラグの期待値を検証
-   既知のV30/8086命令リファレンス値を使ったテーブル駆動テスト

---

## Phase 2 — メモリマップ・割り込みコントローラ・タイマー・DMA

**目的/DoD**: 実際のWonderSwanメモリマップに対してCPUが正しくアクセスでき、
割り込み発生→ベクタジャンプ→IRETの一連の流れが動作する。タイマーが割り込みを
発生させ、GDMA/SDMAによるメモリ/サウンド転送が機能する。

**主要タスク**
-   `MemoryBus`実装: 20-bitアドレス空間、内蔵RAM、I/Oポート空間、カートリッジROM領域へのディスパッチ
-   カートリッジROMバンク切り替えレジスタの最小実装（Phase 6で本格化）
-   割り込みコントローラ: ベクタテーブル、IRQ優先度、INT/IRET命令との連携、VBlank割り込み線の配線
-   ハードウェアタイマー（汎用タイマー、HBlank/VBlankタイマー）のレジスタ実装
-   **DMA(GDMA/SDMA)**: General DMAによるメモリ間転送、Sound DMAによる音声データの
    自動転送ロジック。WonderCrabの`src/dma/gdma.rs` / `src/dma/sdma.rs`の構成を参考にする
-   I/Oポートのリード/ライトをハンドラ関数にディスパッチする仕組み

**依存関係**: Phase 1のCPU実行ループ（特に`MemoryBus`トレイト）に依存。

**テスト方法**
-   ユニットテスト: メモリマップの境界値、割り込みのベクタジャンプ/IRET復帰、タイマーのIRQ発火タイミング
-   DMAユニットテスト: 転送元/転送先/転送長の境界値、転送中のバスアクセス競合

---

## Phase 3 — CPUテストROM検証（v0.1の中核マイルストーン）

**目的/DoD**: 公開されているCPU検証用テストROM（[WSCPUTest](https://github.com/FluBBaOfWard/WSCPUTest)、
[ws-test-suite](https://github.com/asiekierka/ws-test-suite)等）をエミュレータ上で実行し、
テスト結果を自動的に解析してパス判定できる。**これがv0.1のDefinition of Doneの中核。**

**主要タスク**
-   ROMローダー: テストROMをカートリッジ領域にマップして実行開始できる最小限のブートストラップ
-   テストROM実行結果の検証方法の確立: WSCPUTest/ws-test-suiteの出力フォーマット
    （メモリダンプ/I/Oポート出力等）に合わせたテストハーネスを実装
-   不足するカバレッジを補うための自作テストROM（V30MZアセンブリで命令列を書き、
    結果を固定アドレスにダンプする）を`tests/fixtures/`に用意
-   テスト実行ハーネス: `tests/`配下にROMを読み込み、終了条件まで実行し結果を検証する統合テスト

**依存関係**: Phase 1（CPU命令セット）+ Phase 2（メモリマップ・割り込み）完了が前提。

**テスト方法**
-   統合テスト（`crates/core/tests/cpu_test_roms.rs`）としてCIに組み込み、継続的にパスを保証
-   発見されたバグはPhase 1のユニットテストに回帰テストとして追加する

**進捗** (Phase 3 着手中)
-   `crates/core/tests/cpu_test_roms.rs` を新設。`Bus + Cpu` を使った統合テストハーネス
    (`run_code` 関数) を実装し、10件の自作マシンコードテストがCIで通過している:
    算術(ADD/SUB/IMUL)・制御フロー(LOOP/JZ/JNZ)・スタック(PUSH/POP)・文字列命令(REP STOSB/MOVSB)・HLT。
-   ROM bank 0 (`CS=0x2000`, `IP=0x0000` → 物理 `0x20000`) にコードを配置し、
    結果を WRAM (`DS:0x0000`) に書き込んで検証するパターンを確立。
-   **残タスク**: 公開テストROM(WSCPUTest / ws-test-suite)オプトインテストの実装、
    テストカバレッジ拡充（BCD命令群・セグメントオーバーライド境界値・未定義opcode挙動記録）。

---

## Phase 4 — PPU実装（モノクロ、タイルベース2D描画）

**目的/DoD**: 224x144解像度、4階調グレースケールでのタイルベース背景レイヤー+
スプライトレイヤーの描画が正しく行われ、VBlank割り込みと同期してフレームバッファが生成される。

**主要タスク（`crates/core` 内 `ppu`モジュール）**
-   VRAM/タイルデータ/タイルマップ/パレットレジスタのメモリマップ配線
-   背景レイヤー描画ロジック（スクロール、タイルフリップ等）
-   スプライトレイヤー描画ロジック（優先度、X/Yフリップ、オーバーフロー処理）
-   **カラー抽象化ポイント**: パレット解決を`PaletteResolver`トレイトとして切り出し、
    モノクロでは「4階調グレースケール固定変換」を実装、Phase 8でColorのパレット実装に差し替える
-   フレームバッファ出力フォーマットの設計（将来のRGBA統一表現を見据える）
-   HBlank/VBlankタイミングとPPU描画ステップの同期

**依存関係**: Phase 2（I/Oディスパッチ・割り込み・タイマー）。Phase 3完了後に着手。

**テスト方法**
-   ユニットテスト: 既知のタイルデータ+パレット設定から期待されるピクセル出力を比較
-   可能であれば公開PPU検証テストROMを利用、無い場合は自作テストROM+フレームバッファの
    スナップショット比較（ハッシュ化）で回帰検出

**設計上の確定事項（実装着手時に確認済み）**
-   **VRAMは独立メモリではなく内蔵WRAM（0x0000–0x3FFF）を共有する**。タイルデータ・
    タイルマップ・スプライトアテーブル（OAM）はすべてWRAM内に置かれ、PPUは `bus.wram` を
    読んで描画する（WonderCrab `src/display/` 参照）。新規VRAMバッファは作らない。
-   **駆動粒度**: スキャンライン単位で開始（`render_scanline` を1ラインずつ）。ドット単位
    精度は将来のリファクタ余地としてAPI設計で残す。
-   **フレームバッファ形式**: `[u8; 224*144]` のパレット解決済み濃淡インデックス
    （モノクロは最終グレー濃淡 0–15）。RGBA展開は frontend/video 側が担当。
-   **モノクロのパレット連鎖**: タイルピクセル(2bit) → パレット 0x20–0x3F の
    プールインデックス(3bit) → 濃淡プール 0x1C–0x1F の濃淡(4bit) → フレームバッファ。
    この解決を `PaletteResolver` トレイトで抽象化し、Phase 8でColor実装に差し替える。

**サブフェーズ分解（実装単位）**

| サブ | 内容 | 状態 |
|---|---|---|
| 4a | PPUスケルトン: `Ppu`構造体・フレームバッファ(224×144)・`DisplayControl`レジスタアクセサ・`lib.rs`へ`pub mod ppu` | ✅ 完了 |
| 4b | タイルデコード(2bppプレーナー) + 背景レイヤー(SCR1/SCR2)スクロール・フリップ・`render_scanline` | ✅ 完了 |
| 4c | `PaletteResolver`トレイト + `MonoPaletteResolver`（濃淡プール連鎖） | ✅ 完了 |
| 4d | スプライトレイヤー(OAM): 4バイトエントリ・優先度・フリップ | ✅ 完了 |
| 4e | ウィンドウマスク(SCR2 inside/outside・スプライトウィンドウ) | ✅ 完了 |
| 4f | Bus統合(`Bus`が`Ppu`保持) + `render_scanline`/`framebuffer`公開 + スキャンライン同期 | ✅ 完了 |
| 4g | テスト整備: 統合テスト `tests/ppu_render.rs`（public API + CPU→I/O→PPU経路） | ✅ 完了 |

**実装メモ**
-   `render_scanline<R: PaletteResolver>` はジェネリックresolver引数を取り、`Bus::render_scanline`
    が `MonoPaletteResolver` を渡す。Phase 8でColor resolverに差し替え可能。
-   合成順（背→前）: SCR1 → スプライト(priority 0) → SCR2 → スプライト(priority 1)。
-   スプライトウィンドウのinside/outside意味は実機未確認（コードコメントで明記）。
-   PPU内部型（`tile_pixel`/`SpriteEntry`/`TileMapEntry`/`DisplayControl`）は `pub(crate)`。
    crate公開APIは `Ppu`/`SCREEN_WIDTH`/`SCREEN_HEIGHT`/`PaletteResolver`/`MonoPaletteResolver` のみ。
-   テスト数: PPUユニット 61 + Bus統合 6 + `ppu_render.rs` 7 = Phase 4で +74。

**Phase 4 後続課題（DevelopmentPlan §9 リスク管理に紐づく最適化・精度項目）**
1.  **スプライト描画の最適化**: 現状 `render_scanline` はピクセル `x` ごとに priority 0/1 で
    `sample_sprite` を2回呼び、毎回OAM全128件をデコードする（約 224×2×128 デコード/ライン）。
    スキャンライン開始時にOAMを1度走査して該当スプライトを収集する方式へ変更する。
    cycle-accuracy方針上、PPUをドット単位駆動へ移行する際に併せて実施するのが自然。
2.  **スプライト1スキャンライン上限**: 実機は1ラインあたり最大32スプライト。現状は上限なし。
    上記1の収集方式と同時に上限処理（オーバーフロー）を実装する。
3.  **背景色レジスタ**: 全レイヤー透明時の背景色（DISP_CTRL上位ビット/専用レジスタ）が未実装。
    現状は描画なし=shade 0 固定。
4.  **`in_window` の引数整理**: ウィンドウ矩形ポート4つを個別引数で渡している。可読性向上のため
    `WindowRect` 構造体等にまとめることを検討（軽微）。

**対応フェーズ**: 1・2 はPPUドット単位駆動へのリファクタ時（cycle-accuracy強化フェーズ）。
3・4 は任意タイミング（Phase 7のフロントエンド実プレイ前に1・2・3を推奨）。

---

## Phase 5 — APU実装（音声生成）

**目的/DoD**: 矩形波チャンネル、ノイズチャンネル、PCM風チャンネルがレジスタ設定に応じて
正しい波形のサンプル列を生成できる（cpal出力は不要、サンプルバッファをcoreから取得できればよい）。

**主要タスク（`crates/core` 内 `apu`モジュール）**
-   音声レジスタのメモリマップ配線
-   各チャンネルの波形生成ロジック（周波数カウンタ、デューティ比、ノイズLFSR相当、PCM再生）
-   Sound DMA（Phase 2）との連携
-   サンプリングレート変換とサンプルバッファのcore外部への公開API
-   CPUクロックとのサイクル同期

**依存関係**: Phase 2（メモリマップ・DMA）。Phase 4と並行作業可能。

**テスト方法**
-   ユニットテスト: 既知のレジスタ設定から解析的に計算した波形とサンプル列を比較

**設計上の確定事項**
-   **全4チャンネルが波形テーブル方式**（WonderSwanの音源はGB/NESと異なり、4ch全てが
    32サンプル×4bitの波形テーブルオシレータ）。「矩形波」は波形テーブルに矩形パターンを
    書くことで得る。波形データは内蔵WRAMを共有（PPUと同様、独立音声RAMは無い）。
-   **駆動粒度**: 3.072 MHzサウンドクロック（= CPU 1サイクル）単位で `tick(cycles, wram, ports)`
    を進め、128サイクルごとに1ステレオサンプルを生成（24000 Hz）。
-   **出力形式**: インターリーブ ステレオ `i16`（`L, R, L, R, …`）@ 24000 Hz。
    `Bus::audio_samples() -> &[i16]` / `clear_audio_samples()` で公開。最終的なゲイン調整・
    DCセンタリング・ホスト sample-rate へのリサンプルは frontend/audio (cpal) 側が担当。
-   **チャンネル特殊機能**: ch2=ボイス(8bit PCM, port 0x89がサンプル / 0x94がL/R音量)、
    ch3=スイープ(0x8C/0x8D)、ch4=ノイズ(15bit LFSR, port 0x8E, タップ可変)。
    スイープ・ノイズは結果をレジスタ可視状態へ書き戻す（pitch 0x84/85、LFSR 0x92/93）。

**サブフェーズ分解（実装単位）**

| サブ | 内容 | 状態 |
|---|---|---|
| 5a | `apu`モジュール: `WaveChannel`波形ステッピング + `Apu`スケルトン + サンプルバッファ + `lib.rs`へ`pub mod apu` | ✅ 完了 |
| 5b | チャンネルenable + 音量ミックス(L/R nibble) + ステレオ出力 | ✅ 完了 |
| 5c | ノイズ(ch4): 15bit LFSR・タップ可変・0x92/93書き戻し | ✅ 完了 |
| 5d | スイープ(ch3) + ボイス(ch2 PCM, 0x94音量) | ✅ 完了 |
| 5e | Bus統合(`Bus`が`Apu`保持) + `tick_apu`/`audio_samples`/`clear_audio_samples`公開 + 統合テスト `tests/apu_render.rs` | ✅ 完了 |

**実装メモ**
-   WonderCrab `src/sound` をアルゴリズム参照とし、borrow安全・core非依存・解析テスト可能な
    設計に作り替えた。2点の意図的な逸脱（コード内に明記）:
    1.  **ノイズLFSRのシード**: WonderCrabは0シードだが、XORフィードバックでは全0が固定状態
        （ノイズが出ない）。Swaniumは非ゼロ（1）でシードし、reset要求(0x8E bit3)も1で再シード。
    2.  **ノイズDACレベル**: WonderCrabは0xFF/0x00。Swaniumは他チャンネルと同じ4bitドメインに
        揃えるため 0x0F/0x00 を出力。
-   APU内部型 `WaveChannel` は `pub(crate)`、自由関数 `pitch_of`/`mix`/`voice_output` は private。
    crate公開APIは `Apu` と関連定数（`OUTPUT_SAMPLE_RATE`/`CYCLES_PER_SAMPLE`/`MASTER_CLOCK`/
    `STEREO_CHANNELS`）のみ。
-   テスト数: APUユニット 32 + `apu_render.rs` 8 = Phase 5で +40。

**Phase 5 後続課題**
1.  **Sound DMA(SDMA)連携**: ボイスチャンネルへのSDMA転送（port 0x4A–0x52, Phase 2でレジスタ
    配線済み）と `tick_apu` の連動が未実装。現状ボイスは port 0x89 を直接読む。SDMAが
    サンプルを 0x89 へ供給する経路を実装する（Phase 7のフロントエンド実プレイ前に推奨）。
2.  **`tick` のサイクル単位ループ**: `tick` は1サウンドクロックずつループする（1フレーム約5万回）。
    性能上は問題ないが、サンプル境界までのバッチ処理（次のチャンネル前進までの最小公倍数で
    まとめて進める）に最適化する余地がある。cycle-accuracy方針上のドット/サウンド単位駆動
    リファクタ時に併せて検討。
3.  **高品質リサンプル**: 現状は3.072 MHz生成値を128サイクルごとに単純間引き（デシメーション）
    しており、ナイキスト以上の成分はエイリアスする。Blip_Buffer相当の帯域制限リサンプルは
    audio品質フェーズで検討（DoDの「正しい波形のサンプル列」は満たす）。
4.  **マスター音量(0x9E)・ヘッドホン出力(0x91)**: WSCのマスター音量2bit、スピーカー/ヘッドホン
    切替・出力シフトは未実装（現状はステレオL/Rを生のまま出力）。frontend実プレイ時に実装。
5.  **ノイズタップ/スイープ周期の実機検証**: タップ位置テーブルとスイープ8192tick周期は
    WonderCrab由来。実機/他エミュとの突き合わせは精度フェーズで実施。

**対応フェーズ**: 1 はPhase 7前に推奨。2・3 はcycle-accuracy/audio品質強化フェーズ。
4 はPhase 7（フロントエンド）。5 は精度フェーズ。

---

## Phase 6 — カートリッジ/セーブRAM

**目的/DoD**: ROMバンク切り替えが正しく動作し、SRAM/EEPROMへの書き込みがセーブデータとして
永続化できる（ファイルI/Oはfrontend側、coreはバイト列の読み書きAPIを提供）。

**主要タスク**
-   カートリッジヘッダ解析（ROMサイズ、セーブタイプ判定等）
-   ROMバンク切り替えレジスタ実装、`CartridgeMapper`トレイトによる複数マッパー方式への対応設計
-   SRAM/EEPROM実装、**RTCはオプショナルなカートリッジ機能として`Option<Rtc>`で設計**
    （モノクロには存在しないため、Phase 8まで未実装でよいが、インターフェースはここで用意する）
-   セーブデータのシリアライズ/デシリアライズAPI

**依存関係**: Phase 2のメモリマップに依存。

**テスト方法**
-   ユニットテスト: バンク切り替え境界値、SRAM読み書き、複数マッパー種別のテストフィクスチャ

### 設計上の確定事項（Phase 6 実装時）

-   **モジュール構成**: `crates/core/src/bus/cart/` 以下に分割。`header.rs`（フッタ解析）、
    `eeprom.rs`（シリアルEEPROMデバイス）、`rtc.rs`（RTCインターフェーススタブ）、
    `mod.rs`（`Cartridge` 本体・バンキング・セーブデータAPI）。
-   **ヘッダ解析**: ROMイメージ末尾16バイトのフッタ（物理 0xFFFF0–0xFFFFF。リセット時の
    CPU実行開始位置でもある）を `CartridgeHeader::parse` で解析。publisher / color フラグ /
    game_id / version / ROMサイズコード / セーブタイプ / 画面向き / マッパー / チェックサムを取得。
    フッタが無い（ROMが16バイト未満）場合は `None`。
-   **セーブタイプ**: `SaveType { None, Sram(usize), Eeprom(usize) }`。フッタの save コード
    （0x01–0x05=SRAM、0x10/0x20/0x50=EEPROM）から容量を決定。1カートリッジのセーブ媒体は
    SRAM **か** EEPROM のどちらか一方。
-   **マッパー方式**: 当初計画の「`CartridgeMapper` トレイト」は、既知マッパーが Bandai 2001 /
    2003 の2種（閉じた集合）であることから、`dyn` トレイトオブジェクトではなく `Mapper` enum
    でのディスパッチに変更した（Apollo Rust ベストプラクティス Ch.6: 閉じた集合は enum を優先、
    `dyn` はヘテロな開集合向け）。決定的・FFIフレンドリーなコア API 方針（RetroAchievements要件）
    とも整合する。2001 は8ビットバンクレジスタ、2003 は上位バイトポート 0xD1/0xD3/0xD5 を加えた
    16ビットバンクレジスタ。バンクオフセットは OR セマンティクス `(bank << 16) | (addr & 0xFFFF)`
    を媒体長で剰余。
-   **EEPROM**: 93Cxx（Microwire）相当のシリアルEEPROM。READ/WRITE/ERASE と拡張命令
    EWEN/EWDS/WRAL/ERAL を実装。カートリッジEEPROMポート 0xC4–0xC8（データ/コマンドラッチ +
    制御）を配線。コマンドワードは `[start][2bit opcode][address]` 形式で、容量に応じたアドレス
    ビット幅（128B→6、1KiB→9、2KiB→10）を使用。
-   **RTC**: `Option<Rtc>` フィールドとして `Cartridge` に保持。Phase 6 ではインターフェース
    （`Rtc` 型、`state`/`load_state`、`Cartridge::has_rtc`/`rtc()`）のみを公開し、実時間計時・
    BCD レジスタ・ポート 0xCA/0xCB のコマンドプロトコルは Phase 8 で実装する。
-   **セーブデータAPI**: `Bus::save_data() -> &[u8]`（SRAM または EEPROM 内容のゼロコピー参照）、
    `Bus::load_save_data(&[u8])`、`Cartridge::has_save()`。ファイルI/O は frontend 側。

### Phase 6 後続課題

-   **内蔵EEPROM（IEEPROM）**: コンソール側の内蔵EEPROM（ポート 0xBA–0xBE、本体設定/名前保存）は
    未配線。デバイス実装（`Eeprom`）は流用可能。Phase 7 のフロントエンド統合時に配線する。
-   **RTC本体**: 計時ロジック・レジスタ・コマンドプロトコルは Phase 8（Color/Crystal）で実装。
    ヘッダからのRTC自動検出（フッタのRTCビット位置）も実機未検証のため Phase 8 で確定する。
-   **マッパー2003の実機検証**: 2003 の上位バイトバンクポート割り当て（0xD0–0xD5）は WonderCrab
    参照実装に準拠。実カートリッジでの動作確認は未実施。
-   **セーブデータのフレーミング**: 現状 `save_data()` は単一媒体（SRAM xor EEPROM）の生バイト列。
    将来 RTC状態を含む複合セーブが必要になれば、バージョン付きフレーミングを別途設計する。

---

## Phase 7 — 最小フロントエンド（実プレイ可能版、v1.0候補）

**目的/DoD**: Slint UIでROMファイルを開き、wgpu描画+cpal音声出力+gilrs/キーボード入力で
実際にゲームがプレイできる。

**主要タスク**
-   `crates/video`: コアのフレームバッファをwgpuテクスチャに変換、最小限のスケーリング
-   `crates/audio`: コアのサンプルバッファをcpalストリームにリングバッファ経由で供給、音声-映像同期
-   `crates/input`: キーボードマッピング + gilrsでのゲームパッド対応
-   `crates/frontend`: Slint UI骨格（ROM選択、起動、基本設定画面）、メインループ
    （**1フレームずつ呼び出し可能なAPI形状**を採用し、RA対応のフレーム境界フック要件と整合させる）
-   `crates/common`: 設定ファイル読み書き、ログ初期化の本実装

**依存関係**: Phase 1-6完了。

**テスト方法**
-   実ROMでの起動・操作確認（手動QA）
-   CIでは「ビルドが通る」「ヘッドレスでcoreを一定フレーム実行してクラッシュしない」程度の自動テストに限定

---

## Phase 8 — WonderSwan Color / SwanCrystal拡張

**目的/DoD**: Color/Crystalモードでカラー表示（拡張パレット）とRTC機能が動作し、
モノクロモードとの切り替えがROMヘッダ判定または設定で行える。

**主要タスク**
-   `HardwareModel`フラグに応じたPPUのカラーパレット実装差し替え（Phase 4の`PaletteResolver`を実体化）
-   RTC実装（Phase 6で用意したインターフェースを実体化）
-   Color版で拡張されたメモリ/VRAM容量への対応
-   Color版特有のAPU拡張への対応

**依存関係**: Phase 4（PPU抽象化）、Phase 6（RTCインターフェース）完了が前提。

**テスト方法**
-   Color版テストROM（入手できれば）での検証
-   既存モノクロ回帰テストがColorモード追加後も通ることの確認

---

## Phase 9以降 — 将来機能

依存関係の都合上、概ね以下の順で実施する:

1.  **セーブステート**: core全体の状態をシリアライズ可能にする。Phase 1-6で各サブシステムの
    状態をシリアライズしやすい構造にしておくことが前提。
2.  **早送り/巻き戻し**: セーブステートの定期スナップショット機構の上に構築。
3.  **シェーダー対応/LCDシミュレーション**: `crates/video`のwgpuパイプラインにポストプロセス
    シェーダーステージを追加。
4.  **スクリーンショット/動画録画/音声録画**: フレームバッファ・音声バッファのキャプチャ。
5.  **チート対応**: メモリ書き換えフックをcoreのデバッグAPIとして用意。
6.  **デバッガー/メモリビューア/逆アセンブラ**: 実行トレース・ブレークポイント・メモリダンプ用の
    public APIをcoreに用意し、frontendのデバッグウィンドウから利用。
7.  **RetroAchievements (rcheevos) 統合**: [7. RetroAchievements対応を見据えた設計](#7-retroachievements-ra-対応を見据えた設計)
    で確保したフック（安定したメモリ参照API、フレーム境界フック）を使い、rcheevosライブラリと
    連携してアチーブメント判定・ポーリングを実装する。

------------------------------------------------------------------------

# 5. サイクル精度設計の考慮点

-   **CPU実行モデル**: `Cpu::step()`が「1命令」ではなく「1マシンサイクル（クロック）」を進める
    `tick()`を中心に据える。実用上は「命令フェッチ時に総サイクル数を計算し、その間は
    `pending_cycles`をデクリメントしながら他コンポーネントを同期駆動する」モデルを採用する。
-   **マスタークロックによる駆動**: CPU/PPU/APU/タイマー/DMAをすべて共通の「マスタークロックカウンタ」
    基準で駆動する。各コンポーネントは「自分が消費すべきクロック数」を受け取って内部状態を進める
    `advance(cycles: u64)`系APIを持つ。
-   **同期方式**: メインループは「CPUが1ステップ実行→消費したサイクル数を取得→PPU/APU/タイマー/DMAに
    同じサイクル数だけ`advance`を呼ぶ」という逐次駆動方式から始める。将来的に精度要求が上がった場合は
    イベント駆動型スケジューラへの移行も視野に入れるが、初期実装は逐次駆動で十分な精度を確保できると判断する。
-   **割り込みタイミングの正確性**: VBlank割り込み等はPPUが「いつそのクロックに到達したか」を正確に
    検知し、その瞬間にCPU側へIRQをアサートする。CPU側は命令実行中のクロック単位でも割り込み確認
    ポイントを持てるようにAPIを設計する（V30MZの割り込み受理タイミングの正確性に関わるため）。
-   **テストでの検証**: サイクル精度はユニットテストで「Nサイクル経過後の各コンポーネントの状態」を
    検証する形で継続的に保証する。

------------------------------------------------------------------------

# 6. Color拡張抽象化ポイント一覧

| サブシステム | モノクロ実装 | 抽象化方法 | Color実体化先 |
|---|---|---|---|
| PPUパレット | 4階調グレースケール固定変換 | `PaletteResolver`トレイト | Phase 8 |
| カートリッジRTC | 無し（`Option::None`） | `Cartridge`構造体に`rtc: Option<Rtc>`フィールド | Phase 6でインターフェース定義、Phase 8で実体化 |
| ハードウェアモデル判定 | `HardwareModel::Mono`固定 | enum + ROMヘッダ/ユーザー設定判定 | Phase 8 |
| メモリ容量 | モノクロ容量 | サイズをモデルごとの定数/設定値として参照（ハードコード禁止） | Phase 8 |
| VRAM容量/パレットRAM | モノクロ容量 | 同上 | Phase 8 |

------------------------------------------------------------------------

# 7. RetroAchievements (RA) 対応を見据えた設計

[retroachievements.org](https://retroachievements.org) のアチーブメントシステムに将来対応できるよう、
coreの設計段階から以下を要件として織り込む。RA統合の実装自体はPhase 9以降の将来機能だが、
ここで挙げる制約は初期フェーズの設計判断に影響するため明記する。

-   **決定論的実行(determinism)を要件化**: 同一ROM・同一入力シーケンスからは常に同一の内部状態・
    フレーム出力になることを設計上の制約とする。並行性に依存する非決定的な処理や、現実時間
    （壁時計時間）に依存する処理をcore内部に持ち込まない。
-   **安定したメモリ参照API**: core外部から「システムメモリ空間（20-bitアドレス空間）の任意アドレスを
    読む」公開API（例: `read_memory_at(address: u32) -> u8`相当）を用意する。rcheevos等の
    achievementsエンジンはRAM上の値をポーリング/比較する方式が基本のため、内部実装
    （レジスタ配置やタイミング）を変更してもこのAPIのアドレス空間の意味が変わらないように設計する。
-   **フレーム境界フック**: 「1フレーム実行完了」を外部から呼べる単位にしておく（Phase 7の
    メインループ設計と合わせる）。achievementsクライアントは通常フレーム単位でメモリをポーリングする
    ため、coreが「Nフレームまとめて実行」ではなく「1フレームずつ呼び出し可能」なAPI形状を持つことを
    要件にする。
-   **FFI境界を意識したcore API設計**: 将来libretroコアとしてラップする可能性、または直接rcheevos
    ライブラリ(C/Rust binding)と連携する可能性の両方を残すため、coreのpublic APIはグローバル状態を
    持たず、プレーンなデータ型でやり取りする（既存のcore crate設計方針と一致するため追加コストは小さい）。
-   **セーブステートとの関係**: Phase 9のセーブステート機構は、RA側の「リセット検知」「ロード/セーブに
    よる不正防止」の要件とも関係するため、設計時に意識しておく（本格実装はPhase 9以降のRA統合タスクで行う）。

------------------------------------------------------------------------

# 8. テスト戦略詳細

## tests/ ディレクトリ構成案

```
tests/
├── fixtures/
│   ├── cpu/
│   │   ├── self_built/          # 自作テストROM（アセンブリソース + アセンブル済みバイナリ）
│   │   └── public/              # 公開テストROM配置場所（リポジトリには含めず、READMEに
│   │                             # 入手方法・配置手順を記載。配布不可なROMはgitignore対象）
│   ├── ppu/
│   │   └── self_built/
│   └── cartridge/
│       └── mappers/
├── cpu_test_roms.rs              # Phase 3の統合テスト
├── ppu_snapshot_tests.rs         # Phase 4の統合テスト
├── cartridge_mapper_tests.rs     # Phase 6の統合テスト
└── README.md                     # テストROMの入手方法・ライセンス上の注意・配置手順
```

-   **公開テストROMの扱い**: [WSCPUTest](https://github.com/FluBBaOfWard/WSCPUTest)、
    [ws-test-suite](https://github.com/asiekierka/ws-test-suite)等はリポジトリにROM本体をコミット
    しない方針とする。`tests/README.md`に入手元・配置パスの規約を記載する。CIでは、配布可能な
    自作テストROM（アセンブリから自前でビルド）を主軸にし、公開ROMはオプトインのローカルテスト
    （環境変数でパスを指定して実行するテスト）として扱う。
-   **自作テストROM**: V30MZアセンブリでテストパターンを記述し、結果を固定アドレスにダンプ→
    Rust側テストコードがそのメモリ領域を読んで検証、という方式を全フェーズで横展開する。

## CPU命令ユニットテストの方針

-   テーブル駆動形式: `(opcode_bytes, initial_state, expected_state)`の組をテストケースとして列挙し、
    共通のテストランナー関数で実行する
-   フラグ計算は特に網羅性が重要（オーバーフロー/キャリー/ゼロ/サイン/パリティ/Auxiliary Carryの境界値）
-   BCD命令（V30MZ固有の挙動差がある可能性）は個別に重点的なテストケースを用意し、
    コメントで「8086と異なる点」を明記する
-   アドレッシングモード（セグメント:オフセット計算、ラップアラウンド挙動含む）も別途テーブル駆動テストを用意

## CI自動化方針

-   `cargo test --workspace`を全フェーズ共通でCIに組み込む
-   Phase 3以降は`tests/cpu_test_roms.rs`等の統合テストもCI実行対象に追加
-   フロントエンド（Phase 7）のGUI部分は自動テスト困難なため、CIでは「ビルド成功」+
    「ヘッドレスcore実行のスモークテスト」のみとし、実際の操作確認は手動QAチェックリスト運用とする

------------------------------------------------------------------------

# 9. リスクと不確実性への対処方針

-   **WonderSwanは公開資料が少ないハードウェア**: 未解明動作（一部I/Oレジスタの正確な挙動、
    未定義命令の挙動、PPU/APUの境界条件タイミング）に遭遇することを前提に計画する。
    -   対処方針: 不確実な挙動は「現状の実装上の仮定」をコード内コメントと、将来作成を推奨する
        `docs/dev/HardwareNotes.md`に明記し、後から実機テストや追加資料で修正できるようにする。
    -   実機でのテストが可能な場合、テストROMの実行結果を実機と比較することが最も信頼できる
        検証手段になる。実機が無い場合は、[Mesen](https://www.mesen.ca/)・[Ares](https://ares-emu.net/)・
        WonderCrabなど既存実装との比較（ライセンス遵守の上で）を参照する。
-   **V30MZの8086/V30からの差異が不完全に把握される可能性**: BCD命令、一部のシフト/ローテート命令、
    未定義opcodeの挙動などは特に注意が必要。個別にissue化し、テストケースで仮説を明示した上で実装する。
-   **テストROMの入手性**: WonderSwan専用の検証ROMが少ない場合、自作テストROMへの依存度が高くなる。
    可能な限り「ハードウェア資料に基づく期待値の手計算」または「他エミュレータの実装との比較」で
    クロスチェックする運用を推奨する。
-   **精度とパフォーマンスのトレードオフ**: サイクル精度を重視する設計は実行速度面でコストがかかる
    可能性がある。Phase 7の段階でパフォーマンス計測を行い、必要であれば「クリティカルパスのみ
    最適化する」方針を取る。**精度を犠牲にした高速化は採用しない。**
-   **ライセンス・著作権の懸念**: 公開テストROM、商用ROMのテストフィクスチャ利用について、
    配布可能性を個別に確認し、リポジトリには配布不可なバイナリを含めない運用を徹底する。

------------------------------------------------------------------------

# 10. Rust ベストプラクティス 残作業

Phase 1/2 のコードレビュー（Apollo Rust Best Practices Handbook 準拠）で識別された、
後フェーズで対応予定の改善項目を記録する。

## 10.1 テスト: 1テスト1アサーション原則 (Ch. 5.1) ✅ 対応済み (Phase 3 完了後)

### Phase 1/2 着手前の対応（`bus/tests.rs` 5件）

`bus/tests.rs` 内の複数アサーションテスト5件を個別テストに分割した。

| 旧テスト名 | 分割後テスト名 |
|---|---|
| `wram_word_roundtrip` (3) | `wram_16bit_write_reads_back_same_value` / `_low_byte_first` / `_high_byte_second` |
| `iret_restores_ip_cs_and_flags` (3) | `iret_restores_ip_to_next_instruction` / `_cs` / `_interrupt_flag` |
| `int_instruction_jumps_to_ivt_vector` (3) | `int_instruction_sets_ip_from_ivt` / `_cs_from_ivt` / `_clears_if_flag` |
| `cpu_handle_irq_reads_ivt_from_wram` (3) | `handle_irq_sets_ip_from_ivt` / `_clears_interrupt_flag` / `_clears_halted_state` |
| `gdma_transfers_bytes_from_rom_to_wram` (5) | `gdma_copies_byte_{0,1,2,3}_from_rom_to_wram` / `_sets_complete_irq_after_transfer` |

### Phase 3 コードレビュー後の追加対応

Phase 3 レビューで `bus/tests.rs` の残存する複数アサーションテスト12件と
`cpu_test_roms.rs` の2件を追加で分割した（合計 +24 テスト）。

**`bus/tests.rs` 追加分割（+17 テスト）**:

| 旧テスト名 | 件数 | 主な分割後テスト名（抜粋） |
|---|---|---|
| `wram_read_write` | 3 | `wram_write_reads_back_at_base/mid/top_address` |
| `open_bus_returns_0x90_in_unmapped_range` | 2 | `..._at_start/end_of_unmapped_range` |
| `rom_ex_maps_to_last_rom_bytes_at_power_on` | 2 | `rom_ex_maps_first/second_reset_byte_at_power_on` |
| `rom_bank0_register_controls_0x20000_window` | 3 | `rom_bank0_register_bank0/1/2_maps_to_0x20000` |
| `int_cause_clear_port_clears_selected_bits` | 2 | `..._clears_targeted_bit` / `..._leaves_other_bits_intact` |
| `gdma_ctrl_self_clears_on_read` | 2 | `..._first_read_returns_written_value` / `..._second_read_returns_zero` |
| `gdma_does_not_transfer_without_enable_bit` | 2 | `..._returns_zero_cycles` / `..._leaves_destination_unchanged` |
| `vblank_irq_fires_after_on_vblank` | 2 | `..._is_pending_after_on_vblank` / `..._vector_matches_irq_source_index` |
| `pending_irq_only_returns_enabled_sources` | 2 | `..._is_none_when_only_disabled_source` / `..._is_some_when_enabled_source` |
| `hblank_timer_fires_when_counter_reaches_zero` | 4 | `..._not_pending_after_first/second_hblank` / `..._fires_after_period_hblanks` / `..._irq_source_matches` |
| `hblank_timer_reloads_counter_when_auto_reload_set` | 3 | `..._fires_on_first_period` / `..._irq_clears_after_cause_clear` / `..._fires_again_after_reload` |
| `vblank_timer_fires_when_counter_reaches_zero` | 2 | `..._is_not_pending_before` / `..._appears_in_cause_register_when` |

**`cpu_test_roms.rs` 追加分割（+7 テスト）**:

| 旧テスト名 | 件数 | 分割後テスト名 |
|---|---|---|
| `rep_stosb_fills_four_bytes_in_wram` | 5 | `rep_stosb_fills_byte_{0,1,2,3}_in_wram` / `..._does_not_overwrite_byte_beyond_count` |
| `rep_movsb_copies_bytes_within_wram` | 4 | `rep_movsb_copies_{first,second,third}_byte_to_destination` / `..._does_not_overwrite_byte_beyond_count` |

**合計テスト数（Phase 3 完了時点）**: 165（ユニット 138 + 統合 25 + ignored 2）

## 10.2 ドキュメント: 公開 API 全体への `///` doc コメント (Ch. 8.7)

現状、`Registers` のアクセサメソッド群（`get_reg8`・`set_reg8`・`get_reg16`・`set_reg16` 等）、
および `Cartridge` の一部メソッドに `///` doc コメントがない。
将来 `#![deny(missing_docs)]` を有効化する際の事前作業として以下を対応する:

-   `Registers` の全 `pub` メソッドへの `///` 追加
-   `Cpu` の `pub(crate)` に格上げ候補となるヘルパーへの doc 付与
-   `MemoryBus` トレイトのデフォルト実装メソッドへの `# Panics` セクション追加

**対応フェーズ**: Phase 6（カートリッジ/公開 API 安定化）時に `#![deny(missing_docs)]` を
追加するとともに一括対応する。

## 10.3 `Registers` の `Copy` derive サイズ超過 (Ch. 1.2)

`Registers` 構造体は13フィールド × 2バイト = **26バイト** で、Apollo best practices が
推奨する Copy 型の目安（24バイト = 3ワード）を2バイト超えている。`Cpu` 構造体全体では
42バイト超となる。

現状は問題になっていないが、セーブステート・ロールバック実装時（Phase 9）に
大量コピーが発生する可能性があるため、そのタイミングで以下を検討する:

-   `Copy` を外して `Clone` のみにし、`Cpu::snapshot() -> CpuState` のような
    明示的なスナップショットAPIを導入する
-   あるいはサイズが性能ボトルネックにならないことをベンチマークで確認した上で
    `Copy` を維持する（`#[expect(clippy::...)]` に理由を記載）

**対応フェーズ**: Phase 9（セーブステート）。それまでは `Copy` を維持する。

## 10.4 公開 ROM テストの `TODO(issue)` コメント → GitHub Issue 化 (Ch. 8.6)

`crates/core/tests/public_roms.rs` に以下の `TODO(issue):` コメントが2件ある:

-   `wscputest_all_tests_pass`: WSCPUTest の出力フォーマット（結果バイトのアドレス）確認後に
    プレースホルダーアサーションを実際の検証ロジックへ更新する
-   `ws_test_suite_rom_passes`: ws-test-suite の出力規約確認後に同様に更新する

これらは公開 ROM のソースコード確認（またはハードウェア実測）が完了してから対応できる。
GitHub Issue を起票し、コード内の `// TODO(issue):` を `// TODO(issue #NN):` 形式に更新すること。

**対応フェーズ**: Phase 3 残タスク（公開 ROM 入手・出力フォーマット確認後）。
