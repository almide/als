# ALS — 実行時規範（Runtime）

> Last updated: 2026-09-26

プログラム実行の観測規範（エラー終了・文字列補間の表示形・並行コンビネータ）。
参照方法は [strings.md](strings.md) 冒頭と同じ。

## ALS-R1 effect-main のエラー終了形

`effect fn main` が Err で終わるとき、stderr に `Error: <メッセージ>` を1行
出力し **exit code 1** で終了する。Ok 終了は exit 0。パニック級の異常
（ALS-T6 の abort を含む）も同じ `Error:` 接頭辞と exit 1 に統一される。
Contracts: C-035。

## ALS-R2 補間の表示形

文字列補間 `"${v}"` の表示形は型ごとに規範化される:

- **コンテナ**（List/Map/Set/タプル/Option/Result）: Almide リテラル形
  （`[1, 2]`、`("a", 1)`、`some(3)` 等）。ネストも再帰的に同形。
- **レコード/変種**: `TypeName { field: v, … }` — フィールドは**宣言順**。
  anonymous record はフィールド名の辞書順。再帰・ジェネリック ADT は
  インスタンス化ごとに同じ規則。
- **裸の Float**: Display は整数値の `.0` を落とす（`3`）。`float.to_string`
  は保持する（`3.0`）。この2形の区別は規範である。
Contracts: C-008, C-009, C-010, C-011。

## ALS-R3 fan 並行コンビネータの決定性

**受理形**(構文要素方向: `ExprKind::Fan` / `FanBounded` / `FanRace` /
`FanRaceMap` / `FanSettle` / `FanTimeout`): リスト形 `fan.map(xs, (x) =>
…)`、ブロック形 `fan.any { a(); b() }` / `fan.settle { … }`。全形が
**effect fn 文脈必須**。`fan.map` の mapper は **Result を返す契約**
(裸の値は検査時拒否、`(x) => ok(…)` へ誘導 — any/settle の thunk は
auto-wrap、map の mapper はしない)。`fan.settle { a; b }` の返りは
**Result のタプル**(要素数 = thunk 数、ALS-DT4)。

`fan.any`・`fan.map`・`fan.settle` の結果は**リスト順で決定的**(最初に
完了したものではなく、引数リストの先頭から評価した最初の該当)。エラーは
ALS-R1 の統一 abort 形で表面化する。

`fan.race` と `fan.timeout` は 0.42.0 / 0.29.0 でいったん削除された後、
**決定的意味論を得て 0.47.0 で復活した**: race は (spend, index) 辞書式
最小の勝者則(ALS-DT3、C-205 — mapper 形 `fan.race(budget?, xs, f)` を含む)、
timeout は charge site 協調チェックの壁時計期限(ALS-DT5、C-208)。
`fan.bounded(c) { … }` は決定的予算(ALS-DT2、C-204)であり、mapper 形は
持たない。旧 tombstone 裁定(C-004/C-006)はこの復活で SUPERSEDED。
Contracts: C-004, C-005, C-006。

## ALS-R4 非有限浮動小数の定数表示

const 畳み込みで生じた非有限値（inf / -inf / NaN）は名前付き定数として
表示される（`inf`・`-inf`・`NaN`）。ビットパターンや `1e999` 形は不適合。
Contracts: C-012。

## ALS-R5 プロセス環境

`env.args` は argv[0]（プログラム名）を**除いた**引数列を、`process.args`
は argv[0] を**含む**全列を返し(0.59.1 実測: 引数 2 個で env.args は長さ 2、
process.args は長さ 3・先頭がバイナリパス)、それぞれ両ターゲットで一致する。`env.get(name)` はホストプロセスの環境変数を
観測し、存在すれば some(値)、無ければ none を両ターゲットで同一バイトで
返す（wasm は WASI environ + ランナーの環境継承）。`random.int(a, b)` は
WASI entropy 下でも常に [a, b] 範囲内。

`env.os()` と `env.temp_dir()` は等価性則の**唯一の適用除外**である。両者は
実行中のホストを報告するため、native は実 OS 名と実 temp ディレクトリを、
wasm レグは WASI サンドボックス（`wasi` / `/tmp`）を返す。両ターゲットで
一致させることこそが欠陥であり、「今どのプラットフォームか」を問う
プログラムには真を返さねばならない。除外は無制限ではなく、両レグで同一の
決定的不変量（os は閉じた集合 {macos, linux, windows, wasi} の要素、
temp_dir は非空かつ posix ホストでは絶対パス）が証明対象となる。除外は現在
3 関数(env.os・env.temp_dir・fs.temp_dir — C-189)。第四の関数を
除外に加えるには C-189 の statement とその fixture の改訂を要する。

`process.exec_status` と `process.exec_status_timeout` の起動失敗は、
操作名とコマンド名を含む err とする。接頭辞はそれぞれ
`process.exec_status(<quoted cmd>): ` と
`process.exec_status_timeout(<quoted cmd>, <timeout_ms>): ` とし、
続けてホストのエラー説明を付ける。コマンド名は二重引用符で囲み、内部の
引用符・バックスラッシュ・制御文字をエスケープする。引数列は含めない。
存在しない実行ファイルは起動失敗であり、タイムアウトと報告してはならない。
期限が発火した場合の err は従来どおり `exec timed out after <ms>ms` とする。
テスト: `spec/stdlib/process_timeout_test.almd`

`http.start(method, url, body, headers, limits)` は、要求を始めてすぐに呼び出し
ハンドル（`HttpCall`）を返す。上限は呼び出しごとに `limits = { total_ms,
idle_ms }`（ミリ秒、0 は上限なし）で渡す。`total_ms` は `start` から測る壁時計で、
接続・最初の 1 バイトまでの待ち・本文をすべて含む。`idle_ms` はバイトが届く間隔の
上限で、最初の 1 バイトまでの待ちも含む。発火した上限は、自分の名前を示す err
`request timeout: total_ms <n> exceeded` または
`request timeout: idle_ms <n> exceeded` で呼び出しを終わらせる。
`ALMIDE_HTTP_TIMEOUT_SECS` は上限を取らない呼び出しの既定値であり、上限を持つ
呼び出しには効かない。`poll` と `read_new` は待たない。`wait` は呼び出しが終わる
まで待つ。`cancel` と、ハンドルの最後の写しが捨てられることは、走っている呼び出しを
err `request cancelled` で終わらせて接続を閉じる。以後 `read_new` は空文字列を
返し、サーバーは接続が閉じたことを観測する。上限が発火するかどうかはホストの性質で
ある（C-214 と同じ規律）。固定するのは、発火したときの err の形である。wasm
ターゲットでは、埋め込みホスト（`almide run --target wasm`）が `start`・`poll`・
`read_new`・`wait`・`cancel` と `request_stream_with_limits` を native と同じ
観測で提供する。err の文字列、`cancel` で接続が閉じること、ハンドルの最後の写しを
捨てると呼び出しが取り消されることも同じである。単体の成果物
（`almide build --target wasm`）はこの族を持てず、`almide check --target wasm` は
E081 で拒否する。`openai_streaming_call_with_limits` と
`anthropic_streaming_call_with_limits` は native のみである。
テスト: `spec/stdlib/http_call_test.almd`、`spec/embedded_cross/http_call_handle_errs.almd`

`http.serve(port, f)` は埋め込み wasm レーン（`almide run --target wasm`）でも
native と同じ意味で動く。1 回の実行のすべての要求を 1 つのインスタンスが、受理順に
1 件ずつ処理する。main は 1 回だけ走り、native と同じく `http.serve` を呼ぶ。
ホストは `0.0.0.0:<port>` に bind し、解析済みの要求を 1 件ずつゲストに渡す。
ハンドラは main と同じインスタンス・同じヒープで走る。したがって main が `serve`
の前に計算してハンドラが捕捉した値（乱数・時刻）は、その実行のどの要求でも同じで
あり、`serve` の前の main の効果は 1 回だけ起こる。要求ごとにインスタンスを作る
`wasi:http` proxy ホストの形ではない。要求の読み取りと応答の書き出しは両レグで同じ
コードが行う。要求行のメソッドとターゲット、最初のコロンで分けて前後の空白を除いた
ヘッダ行（到着順）、Content-Length の本文（UTF-8、不正バイトは置換文字）を読む。
応答は `HTTP/1.1 <status> <reason>`（固定の理由句表、表にない status は `OK`）、
応答のヘッダ（その順）、`Content-Length`、本文の順に書き、接続を閉じる。
よって status 行・ヘッダ・本文は両レグでバイト一致する。`req_method`・`req_path`・
`req_body`・`req_header`（最初の一致、ASCII 大小無視）・`query_params`（最初の
`?` 以降を `&` で分け、各組を最初の `=` で分け、`=` の無い組は捨て、`+` と `%XX`
を復号し、後のキーが勝つ）は同じ値を返す。ハンドラの `err(m)` は本文
`Internal error: <m>`、`Content-Type: text/plain` の `500` になる。bind の失敗は、
呼び出しがどの位置にあっても実行を中断し、stderr に
`Error: bind failed: <os message>` を書いて終了コード 1 で終わる。`http.serve` は
err を返さない型なので、呼び出し側の `!` は何もしない（この規範以前、native
ランタイムが返す err が現れるのは呼び出しが関数の末尾にあるときだけで、それ以外の
位置ではプログラムはサーバー無しで先へ進んでいた）。サーバーが走る間もレーンは native の
ストリーム規則を保つ。stderr はバッファしないので、stderr の記録は native と行ごとに
一致する。stdout は端末なら書き込みごとに flush し、それ以外は 64 KiB でバッファする。
対象外: 標準の p1 成果物（`almide build --target wasm`）は待ち受けソケットを持たず、
`http.serve` は check 時に拒否される（E081）。`wasi:http/incoming-handler`
コンポーネントの export は別の形であり、この規範は記述しない（どちらも
almide/almide#2659）。
テスト: `spec/serve_cross/http_serve_replay.almd`（終了しないサーバー fixture で、
汎用ランナーは実行しない。実装側のドライバが両レグで起動し、同じ要求列を再生して
応答の生バイトと stderr の記録を比べ、1 回の実行の 2 つの要求で捕捉した乱数が同じで
あることを確かめ、使用中のポートで起動して中断を比べる）

Contracts: C-096, C-112, C-118, C-133, C-189, C-214, C-366, C-367。

## ALS-R6 ファイルシステムのパス解決

wasm の fs ランタイムは起動時に WASI preopen ディレクトリ表を構築し、
絶対パスを最長一致 preopen + 相対残りに解決する（`./` 正規化込み）。
同一パスへの書き込み→読み戻しは native std::fs と同じホストファイルに
到達する（CWD 非依存）。open エラー文言は ALS-T6 系の native 文言規範
に従う。

`fs.list_dir(path)` はディレクトリの**全**エントリを返す。エントリ数に
上限はなく、ホストのバッファ境界（wasm の `fd_readdir` は resumable API で
あり、1 パスで返るのは呼び出し側バッファに収まる分だけ）は観測に現れない。
`.` と `..` は両ターゲットで除外され、順序は**バイト辞書順**に正規化される
（native は `names.sort()`、wasm は同じバイト比較の挿入ソート）。ファイル
システムの readdir 順は観測できない。読み取り失敗は短いリストではなく err。
テスト: `spec/wasm_cross/fs_list_dir_multipass.almd`
Contracts: C-042, C-272。

## ALS-R7 ストリーミング行走査の可謬コールバック

ADR-0006 の 1 ビット可謬性多相は fs のストリーミング行走査にも適用される。
`fs.fold_lines` / `fs.for_each_line` のコールバック本体が `!` を使うとき、
検査器は呼び先を内部キャリア `fs.__fallible_fold_lines` /
`fs.__fallible_for_each_line` に書き換え、呼び出し全体が可謬になる
（`list.map` → `list.__fallible_map` と同型）。キャリアは綴りではない —
ソースが直接名指しすると E043。

規範となる観測量は **コールバック呼び出し列** である。最初の err を返した
行までコールバックが呼ばれ、それ以降の行では**一度も呼ばれない**
(first-err 打ち切り)。err メッセージはコールバックのものがそのまま
呼び出し側の err チャネルに乗る。この 2 つは native ⇄ wasm でバイト同一。

native レッグは加えて**読み取り自体**を失敗行で止める
(`almide_rt_fs_fold_lines_effect` が BufReader ループから return する)。
wasm 自己ホストは C-220 と同じくファイル全体を先に読むため、
「リーダがどこで止まったか」は wasm の観測量ではない — RSS と同じく
native 限定の性質であり、観測可能な約束には含まれない。

区画走査 (`fold_lines_range` / `fold_lines_chunked`) には可謬形が**ない**。
分割走査の「最初の err」はスレッド実行順の観測量になるため、
定義できる打ち切り点が存在しない — 意図的省略として
`tests/fs_streaming_family_gate_test.rs` の行列が固定する。

テスト: `spec/wasm_cross/fs_fallible_stream_callback.almd`（両レッグ）,
`spec/stdlib/fs_streaming_test.almd`（for_each_line セル — native 限定）,
`tests/fs_streaming_family_gate_test.rs`（行列ゲート）。

Contracts: C-274。

## ALS-R8 HTTP レスポンスヘッダの規範

`http` のレスポンス構築族（`response` / `json` / `redirect` / `with_headers` /
`status` / `body` / `set_header` / `get_header`）は**ネットワークに触れない純
データ操作**で、両ターゲットで走る。ヘッダ名の規範は3つ:

- **フィールド名は大小文字を区別しない**（RFC 9110 §5.1）。畳み込みは
  **ASCII のみ**（フィールド名は `token` であり、v0 の
  `eq_ignore_ascii_case` と一致させる — Unicode の `string.to_lower` を
  使ってはならない）。`get_header` は最初の一致を返し、無ければ none。
- **1つのフィールド名につきエントリは高々1つ。** 書き込み（`set_header`・
  `with_headers`）は大小文字を無視して既存エントリの**値をその場で上書き**
  し（名前の綴りは最初に格納されたものを保つ・位置も動かない）、無ければ
  末尾に追加する。ゆえに `get_header(set_header(r, k, v), k') == some(v)`
  は `k` と `k'` が ASCII 大小文字違いなら常に成り立つ。
- **`with_headers` は渡された Map そのもの**（map 順）を返し、
  `Content-Type` を勝手に**播かない**。既定の Content-Type は、名前を与える
  手段が他にない `response`（`text/plain`）と `json`
  （`application/json`）だけが持つ。`redirect` は `Location` のみを持つ。

テスト: `spec/wasm_cross/http_response_headers.almd`,
`spec/stdlib/http_response_test.almd`。
Contracts: C-275。

## ALS-R9 プロセス終了コードの値域

`process.exit(code)` が受理する code は **0..=125** である。この範囲の値は
native・埋め込みホスト・stock WASI ランタイムのいずれでも**その値で**終了する。
計算された引数が範囲外の code になった場合は定義済みの領域エラーであり、stderr に
`Error: exit code must be in 0..=125` を1行出力して **exit 1** で終了する
（全ターゲットで同一バイト）。

上限は恣意的な切り方ではなく、**出荷される成果物が届けられる値の共通部分**である:

| 層 | 運べる値 | 範囲外に何が起きるか |
|---|---|---|
| POSIX `exit(status)` | 下位 8 bit のみ | 親プロセスは `256` を `0` として観測する（黙った切り詰め） |
| シェルの規約 | 0..125 | 126（実行不可）・127（未検出）・128+n（シグナル n）は**シェル自身が生成する**値 |
| WASI preview-1 `proc_exit` | `[0, 126)` | stock ランタイムはホスト trap にする。exit 要求としての情報は残らず、実行時の本物の障害と区別できない |
| component model `wasi:cli/exit#exit-with-code(u8)` | 0..255 | trap しない |

`almide build --target wasm` が出力するのは preview-1 の成果物であり、126 以上は
その成果物では届けられない。したがって 0..=125 は**言語が保証できる範囲**であって、
実装の都合ではない。

この上限は**床であって永続的な切り詰めではない**。既定の成果物が `proc_exit` を
経由しなくなれば 0..=255 が届けられるようになり、範囲は広げられる（拡大は
後方互換であり、逆向きは破壊的である）。

直接呼び出す `process.exit` の引数が範囲外の整数リテラルなら、checker は
引数を指す **E084** で拒否する。括弧・単項マイナス・各基数のリテラルを含み、
モジュールの別名・選択的 import・パイプ経由でも同じ規則を適用する。
`-0` は 0、二重の単項マイナスは正の値として判定する。

```almide check-fail=E084
import process
effect fn main() -> Unit = process.exit(200)
```

変数や算術式などの計算された引数は、このリテラル診断では拒否しない。
関数値を格納した変数への呼び出しも、この診断では追跡しない。
実行時の範囲チェックは引き続き適用される。
同名のユーザ関数やローカル関数値は標準ライブラリの `process.exit` ではなく、
この診断の対象外である。

テスト: `spec/wasm_cross/exit_code_out_of_range.almd`,
`spec/wasm_cross/exit_code_upper_bound.almd`,
`tests/diagnostics/e084-exit-code-domain/broken.almd`,
`tests/diagnostics/e084-exit-code-domain/fixed.almd`,
`tests/exit_literal_check_test.rs`。
Contracts: C-350, C-351。
