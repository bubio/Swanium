# テストROMの運用方針

このディレクトリには、エミュレーターコアの統合テストと、テストに使うROMフィクスチャを配置する。

## ディレクトリ構成

```
tests/
├── fixtures/
│   ├── cpu/
│   │   ├── self_built/   # 自作テストROM（アセンブリソース + アセンブル済みバイナリ）。リポジトリにコミットする。
│   │   └── public/       # 公開テストROMの配置場所。リポジトリにはコミットしない（下記参照）。
│   ├── ppu/
│   │   └── self_built/
│   └── cartridge/
│       └── mappers/
└── README.md
```

## 公開テストROMについて

[docs/dev/DevelopmentPlan.md](../docs/dev/DevelopmentPlan.md) の方針に従い、配布ライセンスが
不明確な公開テストROMはリポジトリにコミットしない。Swanium のローカル開発環境では
外部ストレージ上の共有 fixture パス（`/Volumes/CrucialX6/roms/WonderSwan/...`）を既定値に
している。別の場所に置く場合は環境変数でパスを指定する。

参照先（出典: ローカル参考実装 `WonderCrab` の readme.md）:

- [WSDev Wiki](https://ws.nesdev.org/wiki/WSdev_Wiki)
- [WonderSwan - Sacred Tech Scroll](http://perfectkiosk.net/stsws.html)
- [WonderSwan CPU test (FluBBaOfWard/WSCPUTest)](https://github.com/FluBBaOfWard/WSCPUTest)
- [WonderSwan Timing test (FluBBaOfWard/WSTimingTest)](https://github.com/FluBBaOfWard/WSTimingTest)
- [WonderSwan Hardware test (FluBBaOfWard/WSHWTest)](https://github.com/FluBBaOfWard/WSHWTest)
- [WonderSwan test suite (asiekierka/ws-test-suite)](https://github.com/asiekierka/ws-test-suite)

仕様確認は WSDev Wiki / Sacred Tech Scroll を一次資料として扱い、互換性確認は
WSCPUTest / ws-test-suite の公開テストROMをオプトインで実行して担保する。公開ROMの
合否判定プロトコル（出力アドレス、終了条件、画面出力の期待値など）はテストごとに
ソースまたは資料で確認し、`crates/core/tests/public_roms.rs` に明記する。

これらのROMを使う統合テストはオプトイン（環境変数でROMパスを指定した場合のみ実行）とし、
CIでは自作テストROM（`fixtures/cpu/self_built/`）を主軸にする。

### WSCPUTest の実行例

FluBBaOfWard/WSCpuTest v0.7.1 は上流リポジトリで以下のようにビルドできる。

```sh
nasm -f bin -o WSCpuTest.wsc WSCpuTest.asm
```

生成した `WSCpuTest.wsc` は既定では次の場所に置く。

```text
/Volumes/CrucialX6/roms/WonderSwan/Tests/WSCpuTest/WSCpuTest.wsc
```

別の場所に置く場合は `WS_CPU_TEST_ROM` でパスを指定して実行する。

```sh
cargo test -p swanium-core --test public_roms -- --include-ignored wscputest
```

### ws-test-suite の実行例

asiekierka/ws-test-suite は ROM ごとに出力規約が異なるため、`public_roms.rs` に
ソース確認済みのデコーダを追加した ROM だけを自動判定対象にする。現在の対象は
`mono/cpu/80186_quirks.ws`。

上流リポジトリでビルドした ROM は、既定では次の場所に置く。

```text
/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/mono/cpu/80186_quirks.ws
```

実行:

```sh
cargo test -p swanium-core --test public_roms -- --include-ignored ws_test_suite
```

別の場所に置く場合は `WS_TEST_SUITE_ROM=/path/to/80186_quirks.ws` で上書きする。
複数の ws-test-suite ROM を使う場合も、上流の `src/...` 構造と同じ相対パスで
`/Volumes/CrucialX6/roms/WonderSwan/Tests/ws-test-suite/` 配下に置き、ROM ごとの合否判定規約を
テストコードに明記する。

### WSTimingTest の実行例

FluBBaOfWard/WSTimingTest v0.4.0 は V30MZ 命令タイミングを実機採取値と比較する。
上流リポジトリで以下のようにビルドできる。

```sh
nasm -f bin -o timingtest.ws timingtest.asm
```

生成した `timingtest.ws` は既定では次の場所に置く。

```text
/Volumes/CrucialX6/roms/WonderSwan/Tests/WSTimingTest/timingtest.ws
```

実行:

```sh
cargo test -p swanium-core --test public_roms -- --include-ignored wstimingtest
```

別の場所に置く場合は `WS_TIMING_TEST_ROM=/path/to/timingtest.ws` で上書きする。
現在の自動 oracle は page 0 の Pass 列を対象にしている。追加ページを有効化する場合は、
各ページの行数と既知の許容差を WSTimingTest ソースで確認してから
`crates/core/tests/public_roms.rs` に明記する。

### WSHWTest の実行例

FluBBaOfWard/WSHWTest は interrupt manager、timer、I/O register、window/sprite/LCD、
sound/noise などをメニューから検査する。上流リポジトリで以下のようにビルドできる。

```sh
nasm -f bin -o WSHWTest.wsc WSHWTest.asm
```

生成した `WSHWTest.wsc` は既定では次の場所に置く。

```text
/Volumes/CrucialX6/roms/WonderSwan/Tests/WSHWTest.wsc
```

実行:

```sh
cargo test -p swanium-core --test public_roms -- --include-ignored wshwtest
```

別の場所に置く場合は `WS_HW_TEST_ROM=/path/to/WSHWTest.wsc` で上書きする。
この ROM は 4 Mbit イメージとして `0x40000` から実行されるため、テスト harness では
direct boot 用に 1 MiB イメージへ配置してから起動する。

## 自作テストROMの方針

V30MZアセンブリでテストパターンを記述し、結果を固定アドレスにダンプする。Rust側のテストコードが
そのメモリ領域を読んで期待値と比較する。
