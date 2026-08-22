# DAW Video Recorder 要求仕様・基本設計書

- 文書バージョン: 0.6-draft
- 作成日: 2026-08-22
- 状態: 要件確認中
- 対象プロジェクト: DAW Video Recorder

## 1. 目的

Studio OneをはじめとするDAW上で、DAWの録音・再生・タイムラインと同期してPC接続カメラの映像を録画する。

プラグインを複数挿入した場合は、プラグインインスタンスごとに独立した論理録画セッションを提供する。異なるカメラを個別に録画する場合と、同じカメラ映像を複数の録画として保存する場合の両方をサポートする。

将来は、スマートフォンアプリから低遅延で送信される映像を、PC接続カメラと同じ映像入力として扱えるようにする。

録画成果物は、プロジェクト単位の動画ファイルと同期メタデータとして保存する。録画後の動画編集は本ソフトウェアの責務に含めず、必要に応じて外部の動画編集ソフトウェアを使用する。

## 2. 設計方針

本システムを次の3領域に分割する。

1. DAWプラグイン: C++/JUCEによるVST3
2. カメラ・録画サービス: Rustによる独立プロセス
3. リモートカメラ通信: 将来の別プロジェクトとしてGoを使用

カメラドライバー、動画エンコーダー、ファイルI/Oなどの障害や負荷をDAWプロセスから隔離するため、録画処理はVST3プラグイン内では実行しない。

### 2.1 ライセンス方針

- JUCEは商用ライセンスではなく、JUCEが提供するAGPLv3のオープンソースライセンス条件で使用する。
- 本プロジェクトが新規作成するC++、Rust、Go、IPCスキーマ、ビルドスクリプトおよび関連ドキュメントは、原則として`AGPL-3.0-only`で公開する。
- GitHub上で完全な対応ソースコード、ビルド手順、ライセンス本文、著作権表示を提供する。
- 第三者ライブラリおよびSDKは各ライセンスを維持し、`THIRD_PARTY_NOTICES.md`とSBOMへ記録する。
- 新規依存関係の導入前にAGPLv3とのライセンス互換性を確認する。
- 将来クローズドソースで配布する場合は、JUCE商用ライセンスを含む全依存関係を改めて審査する。これは現在の対象外とする。

AGPLv3は商用利用を禁止するライセンスではない。本方針は「非商用限定」ではなく、ソース公開とAGPLv3条件の遵守を配布条件とする。

## 3. 対象範囲

### 3.1 初期リリースの対象

- VST3プラグインとしてDAWへロードできること
- PC上のローカルカメラを列挙・選択できること
- プラグインごとにRecord Armを設定できること
- DAW録音状態に同期して映像を録画できること
- DAWの再生状態、録音状態、サンプル位置、テンポ、拍子を取得できること
- 複数のプラグインインスタンスを独立した録画セッションとして管理できること
- 異なるカメラの同時録画に対応すること
- 同じカメラから複数の独立した録画ファイルを生成できること
- 映像とDAWタイムラインの対応を示す同期メタデータを保存すること
- 各DAWプロジェクトのルート直下に`Recordings`フォルダを作成し、録画関連ファイルをプロジェクト単位で保存できること
- 録画サービスまたはDAWの異常終了時に、可能な範囲で録画ファイルを復旧できること

### 3.2 将来対応

- スマートフォンカメラからのWebRTC映像入力
- Go/Pion WebRTCによるPC側受信または通信制御
- macOS対応
- VST3以外のAU、AAX、CLAPなどへの対応
- 複数PC間の同期
- DAW音声を含む最終動画の自動生成

### 3.3 初期リリースの対象外

- VST3からDAW本体の録音ボタンを操作すること
- 動画をDAWタイムラインへ自動挿入すること
- DAW上のオーディオイベント編集を動画へ追従させること
- DAW上で動画のカット、削除、移動、トリム、複製、入れ替えを行うこと
- 録画素材から編集後動画を生成すること
- クラウドへの自動アップロード
- インターネット越しの映像中継
- DAW固有の非公開APIを利用した専用トラック種類の追加

## 4. 用語

| 用語 | 意味 |
|---|---|
| プラグインインスタンス | DAWのトラックまたはチャンネルに挿入されたVST3の1個の実体 |
| 録画サービス | カメラ、エンコード、保存を担当するRust製独立プロセス |
| 映像ソース | ローカルカメラ、仮想カメラ、将来のWebRTCカメラなどの入力 |
| 録画セッション | 1つのプラグインインスタンスに対応する論理的な録画単位 |
| Capture Pipeline | 映像ソースを開き、フレームを連続取得する処理 |
| Encoding Profile | 解像度、fps、コーデック、ビットレートなどの組み合わせ |
| Record Arm | DAW録音開始時にそのプラグインの映像録画を開始する設定 |
| DAWサンプル位置 | DAWプロジェクト先頭を基準とするサンプル単位の位置 |
| Project Root | Studio OneのSongファイルを含むプロジェクトディレクトリ。すべての録画成果物の基準パス |

## 5. システム構成

```text
DAW / Studio One（初期検証対象）
 ├─ VST3 Instance A ─┐
 ├─ VST3 Instance B ─┼─ Local IPC ─ Rust Recording Service
 └─ VST3 Instance C ─┘                  │
                                        ├─ SourceManager
                                        │   ├─ Local Camera A
                                        │   ├─ Local Camera B
                                        │   └─ Future WebRTC Source
                                        │
                                        ├─ RecordingSessionManager
                                        ├─ EncoderManager
                                        ├─ FileWriter
                                        └─ RecoveryManager
```

### 5.1 VST3プラグインの責務

- DAWからtransport情報を取得する
- Record Armとユーザー設定を保持する
- カメラ一覧および録画状態をGUIへ表示する
- プラグインインスタンスを録画サービスへ登録する
- transport変化をリアルタイムセーフなキューへ投入する
- 非リアルタイムスレッドから録画サービスへイベントを送信する
- 入力音声を変更せず出力する
- DAWプロジェクトへプラグイン設定を保存・復元する

VST3のオーディオ処理スレッドでは、カメラ操作、エンコード、ファイルI/O、ブロッキングIPC、mutex待ち、動的メモリ確保を行わない。

### 5.2 Rust録画サービスの責務

- 利用可能な映像ソースの列挙
- カメラの排他的なオープンと共有
- 映像フレームの取得およびタイムスタンプ付与
- Record Arm中の映像パイプライン準備
- フレームまたはエンコード済みパケットのリングバッファ保持
- 動画エンコード
- 録画セッションごとのコンテナ生成
- 動画ファイルおよび同期メタデータの保存
- 複数プラグインインスタンスの調停
- ドロップフレームおよび障害の監視
- DAWまたは録画サービス異常終了時のファイル復旧
- VST3 GUIへ状態、プレビュー、エラーを返す

### 5.3 将来のGoコンポーネントの責務

- WebRTC signaling
- スマートフォンとのPeerConnection管理
- RTP/RTCP処理
- ネットワーク統計および再接続
- スマートフォン映像を共通の映像ソースとしてRust録画サービスへ提供

Goコンポーネントとスマートフォンアプリの詳細は別プロジェクトで定義する。

## 6. 機能要件

### 6.1 プラグインインスタンス管理

- `FR-INS-001`: 録画サービスは同時に複数のVST3インスタンスを登録できなければならない。
- `FR-INS-002`: 1つのVST3インスタンスを1つの独立した論理録画セッションとして扱わなければならない。
- `FR-INS-003`: 各インスタンスはユーザー編集可能な表示名を持たなければならない。
- `FR-INS-004`: 各インスタンスは保存用IDと実行時IDを持たなければならない。
- `FR-INS-005`: DAWによるプラグイン複製で保存用IDが重複した場合、録画サービスは異なる実行時IDを発行しなければならない。
- `FR-INS-006`: インスタンスごとにRecord Arm、映像ソース、保存名、同期補正値を保持しなければならない。

### 6.2 映像ソース管理

- `FR-SRC-001`: PC上で利用可能なカメラを列挙できなければならない。
- `FR-SRC-002`: カメラの安定したデバイスID、表示名、利用可能形式を取得できなければならない。
- `FR-SRC-003`: 各VST3インスタンスは映像ソースを個別に選択できなければならない。
- `FR-SRC-004`: 異なるインスタンスが異なるカメラを同時に使用できなければならない。
- `FR-SRC-005`: 複数インスタンスが同じカメラを選択できなければならない。
- `FR-SRC-006`: 同じ物理カメラは録画サービス内で原則1回だけオープンしなければならない。
- `FR-SRC-007`: カメラ切断と再接続を検出し、状態を全利用インスタンスへ通知しなければならない。
- `FR-SRC-008`: 将来のネットワークカメラを追加できる共通`VideoSource`抽象を設けなければならない。

### 6.3 Record Armと録画制御

- `FR-REC-001`: 各プラグインは独立したRecord Armスイッチを持たなければならない。
- `FR-REC-002`: Record ArmがONで、かつDAWが録音状態へ遷移した場合に録画を開始しなければならない。
- `FR-REC-003`: Record ArmがOFFのインスタンスはDAW録音中でも録画を開始してはならない。
- `FR-REC-004`: DAWが録音状態から離脱した場合、対応する録画を停止・確定しなければならない。
- `FR-REC-005`: DAW録音を再開した場合、既存ファイルへ追記せず新しいtakeを作成しなければならない。
- `FR-REC-006`: 各録画セッションは他セッションから独立して開始、停止、エラー終了できなければならない。
- `FR-REC-007`: Record Arm中はカメラを事前起動し、録画開始時の起動遅延を避けなければならない。
- `FR-REC-008`: オフラインレンダリング中は実カメラ録画を開始してはならない。
- `FR-REC-009`: DAW停止、シーク、ループジャンプなどのタイムライン不連続を検出・記録しなければならない。

### 6.4 複数カメラおよびカメラ共有

- `FR-MUL-001`: 異なるカメラを選択したセッションには、カメラごとに独立したCapture Pipelineを作成しなければならない。
- `FR-MUL-002`: 同じカメラを選択したセッションでは、Capture Pipelineを共有しなければならない。
- `FR-MUL-003`: 同じ映像フレームを複数セッションへ分配する際、不要なフレーム全体のコピーを避けなければならない。
- `FR-MUL-004`: 同一カメラかつ同一Encoding Profileでは、可能な限りエンコーダーを共有しなければならない。
- `FR-MUL-005`: Encoding Profileが異なる場合は、必要に応じて別エンコーダーを生成しなければならない。
- `FR-MUL-006`: 同じカメラを共有していても、各VST3インスタンスについて独立した出力ファイルを生成できなければならない。
- `FR-MUL-007`: 1セッションの書き込み遅延やエラーが、Capture Pipelineや他セッションを停止させてはならない。

### 6.5 DAW同期

- `FR-SYN-001`: VST3は各オーディオ処理ブロックの先頭におけるDAW transport情報を取得しなければならない。
- `FR-SYN-002`: 少なくとも録音状態、再生状態、DAWサンプル位置、サンプルレートを取得しなければならない。
- `FR-SYN-003`: ホストが提供する場合、BPM、拍子、ループ情報、PPQ位置、ホスト時刻を取得しなければならない。
- `FR-SYN-004`: 録音開始・終了イベントにはDAWサンプル位置と時刻情報を含めなければならない。
- `FR-SYN-005`: 映像フレームにはPCのmonotonic clockに基づくタイムスタンプを付与しなければならない。
- `FR-SYN-006`: DAW時刻と映像時刻の対応を同期メタデータへ保存しなければならない。
- `FR-SYN-007`: IPCの配送遅延を録画開始時刻として使用してはならない。
- `FR-SYN-008`: カメラ固有の遅延を補正するインスタンス別またはデバイス別の同期オフセットを設定できなければならない。
- `FR-SYN-009`: 長時間録画における映像クロックとオーディオクロックのドリフトを測定・記録できなければならない。

### 6.6 ファイル出力

- `FR-FIL-001`: DAW録音の開始から停止までを1つのtakeとして扱わなければならない。
- `FR-FIL-002`: takeごと、プラグインインスタンスごとに独立した動画ファイルを生成しなければならない。
- `FR-FIL-003`: ファイル名には日時、インスタンス名、take番号のうち設定された項目を含めなければならない。
- `FR-FIL-004`: 動画と同時に同期メタデータを保存しなければならない。
- `FR-FIL-005`: 録画中断時に復旧可能な一時コンテナまたはfragmented containerを使用しなければならない。
- `FR-FIL-006`: 正常停止時にユーザー指定の最終コンテナへ確定しなければならない。
- `FR-FIL-007`: 同名ファイルを無警告で上書きしてはならない。
- `FR-FIL-008`: ディスク容量不足を検出して録画状態へ反映しなければならない。
- `FR-FIL-009`: 既定の保存先を`<Project Root>/Recordings`としなければならない。
- `FR-FIL-010`: `Recordings`フォルダが存在しない場合、安全に自動作成しなければならない。
- `FR-FIL-011`: プロジェクトごとの動画ファイルと同期メタデータを同じ`Recordings`配下へ保存しなければならない。
- `FR-FIL-012`: ホストからProject Rootを取得できない場合、ユーザーが一度選択したProject RootをDAWプロジェクト状態へ保存しなければならない。
- `FR-FIL-013`: Project Rootが未確定の場合、Record ArmをReady状態へ遷移させず、プロジェクト保存またはProject Root選択を要求しなければならない。
- `FR-FIL-014`: 保存先として確定した絶対パスがProject Root配下にあることを検証しなければならない。
- `FR-FIL-015`: 録画中にProject Rootへ書き込めなくなった場合、復旧用領域へ退避し、後から元の`Recordings`へ復元できなければならない。

初期仕様では動画のみを保存し、DAW音声のmuxは未確定事項とする。

### 6.7 GUI

- `FR-GUI-001`: 録画サービスの接続状態を表示しなければならない。
- `FR-GUI-002`: Record Armを操作できなければならない。
- `FR-GUI-003`: 映像ソースを選択できなければならない。
- `FR-GUI-004`: 解像度、fps、コーデックまたは品質プリセットを選択できなければならない。
- `FR-GUI-005`: カメラ映像をプレビューできなければならない。
- `FR-GUI-006`: 待機中、録画中、確定処理中、エラーの状態を表示しなければならない。
- `FR-GUI-007`: 録画時間、出力先、ドロップフレーム数を表示しなければならない。
- `FR-GUI-008`: インスタンス表示名とファイル名prefixを編集できなければならない。
- `FR-GUI-009`: 同じカメラを使用している他インスタンスの存在を表示できることが望ましい。
- `FR-GUI-010`: 現在のProject Rootおよび`Recordings`保存先を表示しなければならない。
- `FR-GUI-011`: 自動取得できない場合にProject Rootを選択できなければならない。

### 6.8 録画結果の参照

- `FR-OUT-001`: 録画サービスは動画takeごとに一意な`take_id`を発行しなければならない。
- `FR-OUT-002`: 録画確定後、VST3 GUIへ動画ファイルと同期メタデータの保存結果を通知しなければならない。
- `FR-OUT-003`: VST3 GUIから対象takeの保存フォルダを開けなければならない。
- `FR-OUT-004`: VST3 GUIは直近のtakeについて、録画日時、長さ、映像ソース、保存先および確定状態を表示できなければならない。
- `FR-OUT-005`: 録画済み動画をDAWタイムラインへ自動配置または編集してはならない。
- `FR-OUT-006`: 同期メタデータは、将来の外部ツールが動画とDAWタイムラインの対応を再利用できる形式で保存しなければならない。

## 7. 録画状態モデル

各録画セッションは次の状態を持つ。

```text
Disconnected
    │ service connected
    ▼
Idle ── source selected ──> Preparing ── ready ──> Armed
                                                     │
                                           DAW recording starts
                                                     ▼
                                                 Recording
                                                     │
                                           DAW recording stops
                                                     ▼
                                                Finalizing
                                                     │
                                                     ▼
                                                   Armed
```

任意の状態から復旧可能な`Error`へ遷移できる。Record Armが解除された場合は、録画中を除き`Idle`へ戻る。録画中にRecord Armを解除した場合の挙動は未確定事項とする。

## 8. 同期方式

### 8.1 使用する時刻系

本システムは次の時刻系を対応付ける。

1. DAWプロジェクトのサンプル位置
2. DAWホスト時刻。ホストが提供しない場合がある
3. PCのmonotonic clock
4. カメラフレームのデバイス時刻。利用できない場合がある

### 8.2 TransportSnapshot

VST3はオーディオ処理ブロックごとに、固定長の`TransportSnapshot`をロックフリーSPSCキューへ投入する。

```text
TransportSnapshot
  runtime_instance_id
  sequence_number
  callback_monotonic_time_ns
  host_time_ns optional
  project_sample_position optional
  sample_rate
  is_playing
  is_recording
  is_looping
  is_offline_rendering
  tempo optional
  time_signature optional
  ppq_position optional
```

キュー満杯時はオーディオ処理を待たせず、最新状態の通知を優先する。録音状態のエッジは通常スナップショットより優先し、欠落を検出できるsequence numberを付与する。

### 8.3 録画開始

Record Arm中はCapture Pipelineを起動し、短時間のリングバッファを保持する。DAW録音開始イベントを受信した録画サービスは、イベントに記録された時刻と映像フレームのタイムスタンプを照合する。

IPCメッセージが遅れて到着した場合も、リングバッファから必要なフレームを取得する。これにより、IPC遅延と録画同期誤差を分離する。

録画中も短時間の確定待ちキューを保持する。DAW録音停止イベントを受信した場合は、イベントに記録された時刻より後の未確定フレームを破棄してから動画を確定する。確定待ち時間を超えるIPC停止については、同期メタデータへ警告を記録する。

### 8.4 キーフレーム

エンコード済み映像を共有する場合、新しいファイルはデコード可能なキーフレームから開始する必要がある。

- エンコード済みGOPのリングバッファを保持する
- 論理開始時刻以前の直近キーフレームからコンテナへ書き込む
- 正確な論理開始時刻を同期メタデータへ記録する
- 必要な場合、録画終了後にremuxまたはtrimする

### 8.5 テンポ変更とシーク

テンポ変更は映像の再生速度を変化させない。テンポとPPQ位置は同期メタデータとして記録する。

DAWサンプル位置の後退または不連続を検出した場合は、同期メタデータへdiscontinuity eventを記録する。ループ録音時のファイル分割方式は未確定事項とする。

## 9. カメラおよびエンコーダー共有

Rust録画サービスは、概念的に次のレジストリを持つ。

```text
SourceRegistry: SourceId -> CapturePipeline
EncoderRegistry: (SourceId, EncodingProfile) -> EncoderPipeline
SessionRegistry: RuntimeInstanceId -> RecordingSession
```

### 9.1 異なるカメラ

異なる`SourceId`には独立したCapture Pipelineを生成する。USB帯域、CPU、GPU、ディスク帯域の上限を監視する。

### 9.2 同じカメラ

同じ`SourceId`を利用するセッションはCapture Pipelineを共有する。同一Encoding Profileではエンコーダーも共有し、エンコード済みパケットをセッション別のmuxerへ分配する。

### 9.3 バックプレッシャー

各セッションは容量制限付きキューを持つ。1セッションのmuxerまたはディスクが遅延しても、共有Capture Pipelineをブロックしてはならない。

キューが満杯の場合は、対象セッションについて次を実施する。

1. ドロップフレームまたはドロップパケットを記録
2. GUIへ警告
3. 継続不能な場合は対象セッションのみエラー終了

## 10. 録画成果物の取り扱い

### 10.1 基本方針

- 1回のDAW録音区間を1つのtakeとして扱う
- takeごとに動画ファイルと同期メタデータを生成する
- 動画ファイルは一般的な動画プレイヤーおよび外部編集ソフトウェアで利用できる形式とする
- 本ソフトウェアは録画済み動画をDAWタイムラインへ自動配置しない
- DAW上のイベント編集を監視せず、録画済み動画を自動編集しない
- 録画確定後の動画ファイルは、明示的な上書き操作なしに変更しない

### 10.2 同期メタデータ

同期メタデータは、録画時点における動画フレーム時刻とDAWサンプル位置の対応を保存する。これは障害調査、同期精度の測定および将来の外部ツール連携に使用できる。

同期メタデータを利用したDAW編集追従機能は、本ソフトウェアの対象外とする。

### 10.3 互換性方針

- 初期リリースの実機検証および動作保証対象はStudio Oneのみとする
- Cubase/Nuendo、REAPERなどは設計上の互換性を維持するが、初期リリースでは実機検証および動作保証を行わない
- DAW標準Video Trackへの自動イベント生成には依存しない
- 専用の動画トラック種類を追加しない

## 11. IPC仕様

### 11.1 基本方針

- Windows初期版ではNamed Pipeを第一候補とする
- プロトコルにはバージョン番号を持たせる
- VST3のオーディオスレッドから直接IPCを呼び出さない
- C++、Rust、将来のGoから扱えるスキーマを採用する
- メッセージ形式はProtocol Buffersを第一候補とし、技術検証後に確定する
- IPCはローカルマシン内に限定する

### 11.2 主な要求メッセージ

```text
Hello / NegotiateVersion
RegisterInstance
UnregisterInstance
ListSources
SelectSource
ConfigureEncodingProfile
SetProjectRoot
SetArmed
TransportSnapshotBatch
SetSyncOffset
RequestPreview
StopPreview
FinalizeSession
Heartbeat
```

### 11.3 主な応答・イベント

```text
InstanceRegistered
SourceListChanged
SourceStatusChanged
SessionStateChanged
RecordingStarted
RecordingStopped
RecordingFinalized
PreviewFrameAvailable
MetricsUpdated
Warning
Error
HeartbeatAck
```

### 11.4 切断時挙動

- VST3が切断した場合、録画中セッションは即時破棄せず猶予時間を設ける
- 猶予時間内に再接続しなければ録画を停止・確定する
- 録画サービスが切断した場合、VST3はDAW処理を継続する
- VST3は録画サービスの再起動および再接続を試行する
- 再接続後はインスタンス設定とRecord Arm状態を再送する

## 12. 永続化

### 12.1 DAWプロジェクトへ保存する状態

- 保存用インスタンスID
- インスタンス表示名
- Record Arm
- 選択した映像ソースの安定ID
- Encoding Profile
- 保存先設定
- ファイル名prefix
- 同期補正値
- GUIサイズなどの表示設定
- Project Rootのホスト取得値またはユーザー指定値

### 12.2 録画メタデータ

各takeについて少なくとも次を保存する。

```text
schema_version
take_id
session_id
persistent_instance_id
runtime_instance_id
instance_label
source_id
source_display_name
encoding_profile
output_video_path
record_started_at
record_stopped_at
start_project_sample
end_project_sample
sample_rate
start_host_time_ns optional
start_monotonic_time_ns
first_frame_monotonic_time_ns optional
sync_offset_ns
last_frame_monotonic_time_ns optional
end_sync_offset_ns optional
tempo_events
time_signature_events
discontinuity_events
frame_count
dropped_frame_count
buffered_frame_count
discarded_tail_frame_count
stop_holdback_exceeded
warnings
finalization_status
```

## 13. ファイル構成

既定の保存先はStudio OneのSongファイルを含むProject Root直下の`Recordings`とする。出力例を次に示す。

```text
MyStudioOneSong/
├─ MyStudioOneSong.song
└─ Recordings/
   └─ 2026-08-22_StudioSession/
      ├─ face-camera_take-001.mp4
      ├─ face-camera_take-001.sync.json
      ├─ hand-camera_take-001.mp4
      ├─ hand-camera_take-001.sync.json
      └─ session.json
```

録画中は一時拡張子または一時ディレクトリを使用し、正常確定後に最終名へ変更する。同名ファイルが存在する場合はtake番号または一意なsuffixを付加する。

同じカメラを複数セッションで録画する場合も、利用者からは独立した動画ファイルとして参照できるようにする。内部でのデータ重複削減は最適化項目とする。

## 14. 非機能要件

### 14.1 リアルタイム安全性

- `NFR-RT-001`: VST3のオーディオ処理でブロッキング処理を行わない。
- `NFR-RT-002`: オーディオ処理でファイル、ネットワーク、カメラ、Named Pipeへアクセスしない。
- `NFR-RT-003`: オーディオ処理で通常のmutexを取得しない。
- `NFR-RT-004`: オーディオ処理で動的メモリ確保を行わない。
- `NFR-RT-005`: 同期キューが満杯でも音声処理を停止させない。

### 14.2 障害分離

- `NFR-ISO-001`: 録画サービスのクラッシュによってDAWをクラッシュさせてはならない。
- `NFR-ISO-002`: カメラドライバーまたはエンコーダーのエラーをDAWプロセスから隔離する。
- `NFR-ISO-003`: DAWの異常終了後も、録画サービスは可能な限り動画を確定する。
- `NFR-ISO-004`: 1セッションの失敗を他セッションへ波及させない。

### 14.3 性能

- `NFR-PERF-001`: Record Arm中のCapture Pipelineを事前起動して録画開始遅延を最小化する。
- `NFR-PERF-002`: 同じカメラとEncoding Profileを使用する場合、キャプチャとエンコードを共有する。
- `NFR-PERF-003`: プレビューは録画品質や録画フレームレートを低下させない。
- `NFR-PERF-004`: 性能上限を超えた場合は、DAW音声より映像プレビューおよび対象録画セッションを優先的に縮退させる。

同期誤差、連続録画時間、最大カメラ数などの数値目標は実機測定後に確定する。

### 14.4 セキュリティ

- `NFR-SEC-001`: 初期版のIPCはローカルマシンからのみ接続可能とする。
- `NFR-SEC-002`: 出力パスを検証し、意図しない既存ファイルを上書きしない。
- `NFR-SEC-003`: 外部から受け取る識別子やファイル名をそのままパスとして使用しない。
- `NFR-SEC-004`: 将来のWebRTC入力では端末認証と暗号化を必須とする。
- `NFR-SEC-005`: 正規化後の録画出力パスが`<Project Root>/Recordings`配下から逸脱する場合、書き込みを拒否する。

### 14.5 オープンソースライセンス

- `NFR-LIC-001`: JUCEをAGPLv3条件で使用しなければならない。
- `NFR-LIC-002`: 本プロジェクトの第一者コードへ`AGPL-3.0-only`のSPDX識別子を付与しなければならない。
- `NFR-LIC-003`: リポジトリ直下へAGPLv3のライセンス本文を配置しなければならない。
- `NFR-LIC-004`: 配布物へライセンス表示と対応ソースコードの取得方法を含めなければならない。
- `NFR-LIC-005`: 直接依存および配布物に含まれる間接依存のライセンス一覧を生成しなければならない。
- `NFR-LIC-006`: AGPLv3と両立しない依存関係を、配布対象バイナリへリンクまたは同梱してはならない。
- `NFR-LIC-007`: FFmpegやコーデックなど、ビルドオプションによってライセンス条件が変化する依存関係を構成単位で監査しなければならない。

## 15. エラー処理

最低限、次のエラーを個別に識別する。

- 録画サービス未起動
- IPCバージョン不一致
- カメラが見つからない
- カメラが他アプリケーションに占有されている
- カメラ切断
- 要求形式をカメラがサポートしていない
- エンコーダー初期化失敗
- GPUエンコーダー使用不能
- ディスク容量不足
- ファイル作成・書き込み失敗
- セッションキューのoverflow
- フレームまたはパケットの欠落
- DAW時刻情報の不足
- オフラインレンダリング検出
- ファイル確定失敗
- Project Rootが未確定
- `Recordings`フォルダの作成失敗
- Project Rootの移動、削除、アクセス権喪失

エラーにはユーザー向けメッセージ、機械可読コード、対象インスタンスまたは映像ソースIDを含める。

## 16. 受入基準

### 16.1 基本録画

- Studio OneでVST3をロードできる
- カメラを選択してプレビューできる
- Record Arm ONでDAW録音を開始すると動画が生成される
- Record Arm OFFでは動画が生成されない
- DAW録音停止後に動画ファイルを正常再生できる
- Project Root直下に`Recordings`が自動作成される
- 動画と同期メタデータが対象プロジェクトの`Recordings`配下へ保存される
- 別のStudio Oneプロジェクトでは別の`Recordings`配下へ保存され、ファイルが混在しない

### 16.2 複数インスタンス

- 2つのVST3インスタンスで異なる2台のカメラを選択し、2つの独立した動画を同時録画できる
- 2つのVST3インスタンスで同じカメラを選択し、カメラを1回だけオープンした状態で2つの独立した動画を生成できる
- 一方のインスタンスのみRecord Armを解除しても、他方の録画に影響しない
- 一方のファイル書き込みを意図的に遅延させても、他方の録画とDAW音声処理が継続する

### 16.3 同期

- 録音開始・終了のDAWサンプル位置が同期メタデータへ記録される
- 既知の映像・音声イベントを用いて開始オフセットを測定できる
- 30分以上の録画でドリフト量を測定できる
- シークおよびループによるタイムライン不連続がメタデータへ記録される

数値による同期精度の合格基準は、最初の技術検証結果をもとに決定する。

### 16.4 障害復旧

- 録画サービスを異常終了させてもStudio Oneと音声処理が継続する
- DAWを異常終了させた場合、録画サービスが録画中ファイルの確定または復旧を試行する
- カメラ切断をGUIへ通知し、他カメラの録画を継続する
- ディスク容量不足時に既存ファイルを破壊せず録画を停止する

### 16.5 ライセンス適合性

- JUCEを商用ライセンスではなくAGPLv3条件でビルドできる
- 第一者コードに`AGPL-3.0-only`のSPDX識別子が付与される
- リポジトリと配布物にAGPLv3本文および対応ソースコードの案内が含まれる
- 第三者依存関係のライセンス一覧とSBOMを生成できる
- CIで禁止または未確認ライセンスを検出した場合、配布ビルドを失敗させる

## 17. 実装計画

### Phase 0: 未確定仕様の決定

- 初期対象OSとStudio Oneバージョン
- 動画コンテナ、コーデック、既定解像度、fps
- DAW音声を動画へ含めるか
- ループ録音とプリロールの挙動
- Studio OneからProject Rootを取得できるか検証し、取得不能時の選択UIを確定

### Phase 1: JUCE/VST3 transport技術検証

- パススルーVST3の作成
- Studio Oneで録音、再生、サンプル位置、BPM、拍子、ループを取得
- 無音、プラグインGUI閉鎖、トラック休止時のコールバック挙動を確認
- オフラインレンダリングを識別
- transportイベントのログとジッターを測定

### Phase 2: Rust録画サービス単体

- カメラ列挙と安定ID
- 1台のカメラのプレビュー
- タイムスタンプ付き録画
- 一時コンテナと正常確定
- 異常終了後の復旧

### Phase 3: 単一VST3との統合

- IPCプロトコル
- インスタンス登録
- カメラ選択
- Record Arm
- DAW録音開始・終了への追従
- 同期メタデータ出力

### Phase 4: 複数インスタンス・複数カメラ

- [x] SessionRegistry
- [x] カメラ別Capture Pipeline
- [x] 同一カメラの共有
- [x] セッション別ファイル出力
- [x] セッション別エラー分離

### Phase 5: エンコーダー共有と同期精度

- [x] Encoding Profile単位のEncoderRegistry
- [x] GOPリングバッファ
- [x] セッション途中開始
- [x] ハードウェアエンコーダーとソフトウェアfallback
- [x] オフセット測定と補正
- [x] 長時間ドリフト計算の自動試験
- [ ] Studio Oneを用いた実時間30分以上のドリフト受入試験

### Phase 6: 配布品質

- 自動起動と再接続
- インストーラー
- ログ収集
- 対象Studio Oneバージョンでの互換性試験
- AGPLv3ライセンス本文、ソースコード案内、第三者通知の同梱
- C++、Rust、Go依存関係のライセンス監査とSBOM生成
- コード署名

### Phase 7: スマートフォン連携

- Go/Pion WebRTC受信PoC
- 共通VideoSourceへの接続
- 端末認証
- ネットワーク時刻対応
- 再接続と品質適応

## 18. 想定リポジトリ構成

```text
daw-video-recorder/
├─ LICENSE                 # GNU AGPL v3本文
├─ THIRD_PARTY_NOTICES.md  # 第三者ライセンス・著作権表示
├─ SBOM/                   # 生成済みSPDX/CycloneDX文書
├─ CMakeLists.txt
├─ cmake/
├─ plugin/                 # C++/JUCE VST3
├─ recorder/               # Rust録画サービス
├─ protocol/               # IPCスキーマ
├─ shared-test-vectors/    # C++/Rust共通のプロトコル試験データ
├─ tools/                  # 同期測定・復旧ツール
├─ installer/
└─ docs/
```

## 19. 未確定事項

以下は実装開始前または各Phase開始前に決定する。

1. 初期対象をWindows 11 x64のみに限定するか
2. 対象Studio Oneのバージョンとエディション
3. 高品質・低圧縮の動画と低容量H.264/H.265のどちらを優先するか
4. 既定の解像度とfps
5. 動画ファイルへDAW音声を含めるか
6. ループ録音時にループごとにファイルを分割するか
7. プリロールまたはカウントイン部分を動画に含めるか
8. 録画中にRecord Armを解除した場合、即時停止するかDAW録音停止まで継続するか
9. 同じ映像を複数セッションで使用した場合、常に物理的に別ファイルを作るか、参照方式も許可するか
10. 許容する開始同期誤差と長時間ドリフト
11. 同時カメラ数および同時録画セッション数の最低保証値
12. GUIプレビューのサイズと目標フレームレート

## 20. 参考資料

- [JUCE AudioPlayHead](https://docs.juce.com/master/classjuce_1_1AudioPlayHead.html)
- [JUCE AudioPlayHead::PositionInfo](https://docs.juce.com/master/classjuce_1_1AudioPlayHead_1_1PositionInfo.html)
- [Steinberg VST3 ProcessContext](https://steinbergmedia.github.io/vst3_doc/vstinterfaces/structSteinberg_1_1Vst_1_1ProcessContext.html)
- [Steinberg IProcessContextRequirements](https://steinbergmedia.github.io/vst3_doc/vstinterfaces/classSteinberg_1_1Vst_1_1IProcessContextRequirements.html)
- [JUCE Framework](https://github.com/juce-framework/JUCE)
- [JUCE Framework License](https://github.com/juce-framework/JUCE/blob/master/LICENSE.md)
- [JUCE Licensing FAQ](https://juce.com/get-juce/)
- [GNU Affero General Public License v3](https://www.gnu.org/licenses/agpl-3.0.html)
- [Pion WebRTC](https://github.com/pion/webrtc)
