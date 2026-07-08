# V30MZ (µPD70116) 命令サイクルタイミング

WonderSwan の CPU は NEC **V30MZ**（µPD70116 系）です。Intel 8086 とソース互換ですが、
**多くの命令を 8086 より大幅に少ないクロックで実行します**（例: `MOV`=1, `INC`=1,
`IRET`=10、対して 8086 は 10 / 3 / 32）。フレームあたりのクロック予算は固定
（159 ライン × 256 = 40704 clk/frame）なので、命令を過大課金すると1フレームで
実行できる命令数が少なくなり、割り込み多用のゲームが破綻します
（FF4 オープニングの行スクロール割り込みで顕在化。`docs/dev/Status.md` / PR #4 参照）。

このドキュメントは、実装（[`crates/core/src/cpu/timing.rs`](../../crates/core/src/cpu/timing.rs)
と [`crates/core/src/cpu/mod.rs`](../../crates/core/src/cpu/mod.rs)）で採用した
V30MZ サイクル値の一覧と、その出典・解釈・近似の記録です。

## 出典

- **主権威**: WonderSwan 実機準拠のハードウェアリファレンス
  **"Sacred Tech Scroll" — CPU (V30MZ) セクション**
  <http://perfectkiosk.net/stsws.html#cpu>
  実機検証済みで命令ごとのクロック数が直接記載されている。
- **公開 ROM 実測**: FluBBaOfWard/WSTimingTest v0.4.0。Pink
  WonderSwan Color で採取された期待値を持つ公開 timing ROM。Milestone 13
  では page 0 を opt-in oracle として追加し、`NOP`、固定/可変 port I/O、
  taken branch、odd-address branch penalty、`LOOP` taken を照合した。
- **クロスチェック**: NEC µPD70116 データシート
  （`docs/dev/UPD70116.pdf` / `docs/dev/0900766b8002a666.pdf`。
  著作物のためリポジトリ本体には含めず作業ツリーにのみ保持）。

> 注: 本エミュレータは現状「1命令あたりの総クロック数」を返す粒度で、
> プリフェッチキュー（BIU）やメモリウェイトを clock 単位で厳密にモデル化はしていない
> （将来フェーズの精緻化対象）。下表の値はそれらを1命令の合計に畳み込んだもの。

## 表記の約束（stsws 由来）

- **`reg` / `mem` 列**: レジスタオペランドとメモリオペランドで異なる場合の値。
  stsws は `1-3` のようなレンジで示し、**下限=レジスタ、上限=メモリ**。
  本実装のヘルパ `timing::rm(operand, reg, mem)` がこれを選択する。
- **`N+`（分岐）**: stsws は条件分岐/ループを `N+` と記す。
  `N` = 分岐**不成立**時のクロック。WSTimingTest page 0 に合わせ、
  本実装では **`Jcc` 不成立 = 1、成立 = 5、成立先が奇数アドレスなら +1**、
  `LOOP` 成立 = 6 と解釈している。
- **割り込み受理**: ハードウェア IRQ の受理・ディスパッチ（PSW/PS/PC push＋ベクタ
  ロード）のコスト。V30 の軟件 `INT`（9〜10）に準じて **`IRQ_ACK` = 10** とした
  （8086 の ~32/51 は適用しない）。

---

## サイクル表

### データ移動

| 命令 | オペコード | reg | mem | 備考 |
|---|---|--:|--:|---|
| `MOV r/m, reg` / `reg, r/m` | 88–8B | 1 | 1 | V30 は memory でも 1 |
| `MOV r/m, imm` | C6 / C7 | 1 | 1 | |
| `MOV reg, imm` | B0–BF | 1 | – | |
| `MOV acc, [addr]` / `[addr], acc` | A0–A3 | – | 1 | 直接アドレッシング |
| `MOV r/m, Sreg` / `Sreg, r/m` | 8C / 8E | 1 | 3 | stsws: 1–3 |
| `LEA reg, m` | 8D | 1 | – | メモリアクセスなし |
| `LES` / `LDS reg, m` | C4 / C5 | – | 6 | |
| `XCHG r/m, reg` | 86 / 87 | 3 | 5 | |
| `XCHG AX, reg` | 91–97 | 3 | – | |
| `PUSH reg` | 50–57 | 1 | – | |
| `PUSH Sreg` | 06/0E/16/1E | 2 | – | |
| `PUSH r/m` | FF /6 | 1 | 2 | |
| `PUSH imm` | 68 / 6A | 1 | – | |
| `PUSHF` | 9C | 2 | – | |
| `PUSHA` | 60 | 9 | – | stsws: PUSH R = 9 |
| `POP reg` | 58–5F | 1 | – | |
| `POP Sreg` | 07/17/1F | 3 | – | |
| `POP r/m` | 8F | 1 | 3 | |
| `POPF` | 9D | 3 | – | |
| `POPA` | 61 | 8 | – | stsws: POP R = 8 |
| `LAHF` | 9F | 2 | – | |
| `SAHF` | 9E | 4 | – | |
| `XLAT` | D7 | 5 | – | |
| `CBW` | 98 | 1 | – | |
| `CWD` | 99 | 1 | – | |

### 算術・論理

| 命令 | オペコード | reg | mem | 備考 |
|---|---|--:|--:|---|
| `ADD/OR/ADC/SBB/AND/SUB/XOR r/m,reg` 他 | 00–3D, 80/81/83 | 1 | 3 | 書き戻しあり（RMW） |
| `CMP r/m, reg` / 群の CMP | 38–3D, 80/81/83 /7 | 1 | 2 | 読み取りのみ |
| `ALU acc, imm` | 04/05, 0C/0D, … | 1 | – | |
| `TEST r/m, reg` | 84 / 85 | 1 | 2 | 読み取りのみ |
| `TEST r/m, imm` | F6/F7 /0,1 | 1 | 2 | |
| `TEST acc, imm` | A8 / A9 | 1 | – | |
| `INC` / `DEC reg16` | 40–4F | 1 | – | CF 不変 |
| `INC` / `DEC r/m` | FE, FF /0,1 | 1 | 3 | |
| `NEG r/m` | F6/F7 /3 | 1 | 3 | |
| `NOT r/m` | F6/F7 /2 | 1 | 3 | |
| `MUL` / `IMUL r/m` | F6/F7 /4,5 | 3 | 4 | |
| `IMUL reg, r/m, imm` | 69 / 6B | 3 | 4 | |
| `DIV`（符号なし） | F6/F7 /6 | 15 | 24 | |
| `IDIV`（符号付き） | F6/F7 /7 | 17 | 25 | |
| `AAM` (CVTBD) | D4 | 17 | – | |
| `AAD` (CVTDB) | D5 | 6 | – | |
| `DAA` (ADJ4A) | 27 | 10 | – | |
| `DAS` (ADJ4S) | 2F | 10 | – | |
| `AAA` (ADJBA) | 37 | 9 | – | |
| `AAS` (ADJBS) | 3F | 9 | – | |

### シフト・回転（ROL/ROR/RCL/RCR/SHL/SHR/SAR）

| 形式 | オペコード | reg | mem |
|---|---|--:|--:|
| by 1 | D0 / D1 | 1 | 3 |
| by CL | D2 / D3 | 3 | 5 |
| by imm8 | C0 / C1 | 3 | 5 |

### 制御転送

| 命令 | オペコード | 値 | 備考 |
|---|---|--:|---|
| `JMP near` (rel16) | E9 | 4 | |
| `JMP short` (rel8) | EB | 4 | |
| `JMP far` (ptr16:16) | EA | 7 | |
| `JMP r/m near` | FF /4 | 4 / 5 | reg / mem |
| `JMP m far` | FF /5 | 9 | |
| `Jcc` (rel8) | 70–7F | 1 / 5 | 不成立 / 成立。奇数アドレスへの分岐は +1 |
| `CALL near` (rel16) | E8 | 5 | |
| `CALL far` (ptr16:16) | 9A | 10 | |
| `CALL r/m near` | FF /2 | 5 / 6 | reg / mem |
| `CALL m far` | FF /3 | 12 | |
| `RET near` | C3 | 6 | |
| `RET near imm16` | C2 | 6 | |
| `RETF` | CB | 8 | |
| `RETF imm16` | CA | 9 | |
| `IRET` (RETI) | CF | 10 | 8086 は 32 |
| `LOOP` (DBNZ) | E2 | 2 / 6 | 不成立 / 成立 |
| `LOOPE` (DBNZE) | E1 | 3 / 7 | 不成立 / 成立 |
| `LOOPNE` (DBNZNE) | E0 | 3 / 7 | 不成立 / 成立 |
| `JCXZ` | E3 | 1 / 4 | 不成立 / 成立 |
| `INT n` | CD | 10 | 命令側で総コスト計上 |
| `INTO` | CE | 6 / 13 | OF=0 / OF=1 |
| `ENTER` (PREPARE), level 0 | C8 | 8 | level>0 は未実装 |
| `LEAVE` (DISPOSE) | C9 | 2 | |
| `BOUND` (CHKIND) | 62 | 13 | stsws: 13–20 |
| `HALT` | F4 | 9 | |
| `NOP` | 90 | 1 | WSTimingTest page 0 confirmed |
| `CLC/STC/CMC/CLD/STD/CLI/STI` | F5, F8–FD | 4 | フラグ操作各 4 |

### ポート I/O

| 命令 | オペコード | 値 |
|---|---|--:|
| `IN` / `OUT acc, imm8` | E4–E7 | 7 |
| `IN` / `OUT acc, DX` | EC–EF | 5 |

### 文字列命令（単発）

| 命令 | オペコード | 値 | 備考 |
|---|---|--:|---|
| `MOVS` (MOVBK) | A4 / A5 | 5 | |
| `CMPS` (CMPBK) | A6 / A7 | 6 | |
| `STOS` (STM) | AA / AB | 3 | |
| `LODS` (LDM) | AC / AD | 3 | |
| `SCAS` (CMPM) | AE / AF | 4 | |
| `INS` (INM) | 6C / 6D | 6 | |
| `OUTS` (OUTM) | 6E / 6F | 7 | |

`REP`/`REPE`/`REPNE` プレフィックス付きは、本実装では上表のベース値を**1要素あたりに近似計上**
している（`exec_string_op` のループ）。stsws は「セットアップ + N×要素」形式の別モデルを示すが、
現状は簡易近似（`timing` モジュールにも明記）。

### 割り込み受理

| 事象 | 値 | 実装 |
|---|--:|---|
| ハードウェア（マスカブル）IRQ 受理 | `IRQ_ACK` = 10 | `Cpu::handle_irq` が返し `System::run_cpu_cycles` が加算 |

軟件 `INT` / `INTO` / `BOUND` / 0除算（INT0）は、それぞれの**命令が自前で総コストを返す**ため、
`handle_irq` の戻り値は無視する（二重計上を避ける）。

---

## 実装上の近似・未検証事項

正確性のため、8086 との差や割り切った点を明記しておく（将来の精緻化・実機照合の起点）。

1. **読み取り専用メモリ形の1クロック**: stsws は ALU 系を `1-3` のレンジで示し、
   書き戻し(RMW)と読み取り専用を区別しない。本実装は
   - 書き戻しあり（`ADD` 等の `r/m,reg` 形、即値群の非CMP）: mem = **3**
   - 読み取りのみ（`op reg, r/m` の source 形、`CMP`、`TEST`）: mem = **2**
   と分けている。これはレンジ内の妥当な解釈だが、実機の厳密値は未照合。
2. **REP 文字列**: 上記の通り「ベース値 × 反復回数」の近似。実機は反復のセットアップ/
   要素あたりコストが分かれる。ホットループで支配的になる場合は要精緻化。
3. **プリフェッチ/バス幅/ウェイト非モデル化**: 1命令合計に畳み込み。内蔵 RAM と
   カートリッジ ROM のアクセスコスト差、ワードアクセスの整列ペナルティ等は未反映。
4. **プレフィックス**: セグメントオーバライド/`REP` プレフィックス自体の 1〜数クロックは
   内側命令のコストに含めず（内側命令を再帰実行してそのコストを返す）。実機では
   プレフィックスに小さな追加コストがあるが、支配項ではないため未加算。
5. **分岐成立ペナルティ**: WSTimingTest page 0 により、taken `Jcc` は 5、
   奇数アドレスへの分岐は +1、taken `LOOP` は 6 としている。その他の
   分岐・call/jump 系はページ追加検証の対象。
6. **`BOUND`/`ENTER`（level>0）等の可変コスト**: stsws のレンジ下限〜代表値を採用。
   `ENTER` の level>0 は命令自体が未実装。

## コードとの対応

- 定数・ヘルパ・出典コメント: `crates/core/src/cpu/timing.rs`
  （`IRQ_ACK`、`rm(operand, reg, mem)`）。
- 各命令の返却値: `crates/core/src/cpu/mod.rs` の `execute` / `exec_alu_form` /
  `exec_string_op`、および `Cpu::cycles_for`。
- 検証テスト: `crates/core/src/cpu/tests/`（`alu.rs` / `bit_ops.rs` /
  `ctrl_flow.rs` / `mov_stack.rs` がサイクル数を assert。`ctrl_flow.rs` に
  `handle_irq` 受理コストと軟件 `INT` の非二重計上テスト）。
