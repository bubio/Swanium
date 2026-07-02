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
不明確な公開テストROMはリポジトリにコミットしない。`tests/fixtures/cpu/public/` 配下に各自で
配置して使う想定（`.gitignore`によりバイナリは無視される）。

参照先（出典: ローカル参考実装 `WonderCrab` の readme.md）:

- [WSDev Wiki](https://ws.nesdev.org/wiki/WSdev_Wiki)
- [WonderSwan - Sacred Tech Scroll](http://perfectkiosk.net/stsws.html)
- [WonderSwan CPU test (FluBBaOfWard/WSCPUTest)](https://github.com/FluBBaOfWard/WSCPUTest)
- [WonderSwan test suite (asiekierka/ws-test-suite)](https://github.com/asiekierka/ws-test-suite)

仕様確認は WSDev Wiki / Sacred Tech Scroll を一次資料として扱い、互換性確認は
WSCPUTest / ws-test-suite の公開テストROMをオプトインで実行して担保する。公開ROMの
合否判定プロトコル（出力アドレス、終了条件、画面出力の期待値など）はテストごとに
ソースまたは資料で確認し、`crates/core/tests/public_roms.rs` に明記する。

これらのROMを使う統合テストはオプトイン（環境変数でROMパスを指定した場合のみ実行）とし、
CIでは自作テストROM（`fixtures/cpu/self_built/`）を主軸にする。

## 自作テストROMの方針

V30MZアセンブリでテストパターンを記述し、結果を固定アドレスにダンプする。Rust側のテストコードが
そのメモリ領域を読んで期待値と比較する。
