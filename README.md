# DAW Video Recorder

**DAW Video Recorder** は、DAW（Studio One 等）のトランスポート（録音・再生・停止）と高精度に同期して、Web カメラや HDMI キャプチャデバイスの映像を自動録画するオープンソースシステムです。

音声制作時の演奏・解説動画の収録において、録画開始の手間や後工程での音ズレ・同期合わせ作業をゼロにすることを目的としています。

---

## 主な機能

- **DAW 完全同期録画**: DAW で録音を開始すると自動でカメラ録画が始まり、DAW を停止すると即座にテイク動画（MP4）と同期メタデータ（JSON）が確定保存されます。
- **リアルタイム映像プレビュー**: プラグイン画面内でカメラ映像をアスペクト比維持（レターボックス/ピラーボックス）でプレビュー。録画中点滅バナー（`● REC`）、経過タイムコード、推定フレームレート（fps）をオーバーレイ表示。
- **プラグイン画面と連動した省電力設計**: プラグインエディタを閉じている間はプレビューストリームを自動停止し、DAW のオーディオ処理やシステムへの負荷を最小化。
- **GPU ハードウェアエンコード**: Windows Media Foundation を活用した NVENC / Intel QuickSync / AMD AMF 経由の高速 H.264 エンコードに対応。
- **マルチカメラ & カメラ共有**: 複数のプラグインインスタンスを立ち上げて異なるカメラを同時収録可能。また、複数のトラックで同一カメラを共有しても、エンコード処理を 1 つに統合して低負荷で並行記録。
- **高精度タイミング同期メタデータ**: 各テイクごとに、DAW トランスポート基準時間、カメラフレームのモノトニック時刻、クロックドリフト補正情報を含む `.sync.json` を出力。
- **ワンクリック保存先オープン**: 確定したテイクの保存先フォルダをエクスプローラーで即座に開く `Open Folder` ボタンを搭載。

---

## システム構成

```
┌────────────────────────────────────────────────────────┐
│ Studio One / DAW Host (VST3 Process)                   │
│                                                        │
│  [Audio Thread]                                        │
│      └─► SPSC Lock-free Queue                          │
│  [Background Worker / ServiceClient]                  │
│      └─► Named Pipe IPC (\\.\pipe\daw-video-recorder-v2)│
│  [GUI / PluginEditor]                                  │
│      └─► VideoPreviewComponent (JUCE 2D Graphics)      │
└──────────────────────────┬─────────────────────────────┘
                           │ Protocol Buffers IPC
┌──────────────────────────▼─────────────────────────────┐
│ daw-video-recorder-service.exe (Rust Process)          │
│                                                        │
│  - Multi-Client Instance Manager                       │
│  - Media Foundation Camera Capture & Preview Streamer  │
│  - Hardware H.264 Encoder (GOP Ring Buffer)            │
│  - MP4 Muxer & .sync.json Metadata Writer              │
└────────────────────────────────────────────────────────┘
```

---

## 動作環境・必要要件

- **OS**: Windows 10 / 11 (64-bit)
- **DAW**: VST3 プラグインに対応した DAW（Presonus Studio One 等）
- **ビルドツール**:
  - Rust 1.80 以上 (stable toolchain)
  - CMake 3.25 以上
  - Visual Studio 2022（「C++ によるデスクトップ開発」ワークロード）
  - JUCE 7/8（`third_party/JUCE` または `DAWVR_JUCE_DIR` で指定）

---

## ビルド手順

### 1. リポジトリのクローン

```powershell
git clone --recurse-submodules https://github.com/hiro-1219/daw-video-recorder.git
cd daw-video-recorder
```

### 2. Rust 録画サービスのビルド & テスト

```powershell
# 単体テストの実行
cargo test --workspace

# リリースビルド
cargo build --release -p daw-video-recorder-service
```
生成物: `target/release/daw-video-recorder-service.exe`

### 3. C++ VST3 プラグインのビルド

```powershell
# CMake の構成 (JUCE を明示する場合は -DDAWVR_JUCE_DIR=... を追加)
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release

# プラグインおよびスモークテストツールのビルド
cmake --build build --config Release
```
生成物:
- VST3 プラグイン: `build/plugin/DAWVideoRecorderPlugin_artefacts/Release/VST3/DAW Video Recorder.vst3`
- スモークテストツール: `build/tools/ipc-smoke/DAWVideoRecorderIpcSmoke_artefacts/Release/DAWVideoRecorderIpcSmoke.exe`

---

## インストール・セットアップ

### 1. 録画サービス実行ファイルの配置
VST3 プラグインから自動検出・起動できるように、以下のいずれかの方法で配置します：

- **標準配置場所（推奨）**:
  `target/release/daw-video-recorder-service.exe` を `%LOCALAPPDATA%\DAW Video Recorder\` にコピーします。
  ```powershell
  New-Item -ItemType Directory -Force -Path "$env:LOCALAPPDATA\DAW Video Recorder"
  Copy-Item "target\release\daw-video-recorder-service.exe" "$env:LOCALAPPDATA\DAW Video Recorder\"
  ```
- **環境変数での指定（開発時）**:
  環境変数 `DAWVR_SERVICE_PATH` に実行ファイルのフルパスを設定します。

### 2. VST3 プラグインの配置
ビルドされた `DAW Video Recorder.vst3` を Windows の標準 VST3 ディレクトリにコピーします：

```powershell
# 標準 VST3 フォルダにコピー
Copy-Item -Recurse "build\plugin\DAWVideoRecorderPlugin_artefacts\Release\VST3\DAW Video Recorder.vst3" "C:\Program Files\Common Files\VST3\"
```

---

## 使用方法

### 1. Studio One でのセットアップ
1. Studio One を起動し、トラックのインサートエフェクト（またはマスター/Listen バス）に **「DAW Video Recorder」** を追加します。
2. プラグイン画面が開くと、バックグラウンドで録画サービスが自動起動・接続されます（ヘッダー右上に緑色で `Connected` と表示されます）。
3. **Camera Source**: 使用したいカメラ（Web カメラ、キャプチャボード等）を選択します。左ペインにリアルタイムプレビューが表示されます。
4. **Project Root Folder**: `Browse...` ボタンを押し、プロジェクトの保存先フォルダを指定します（テイク動画は `<ProjectRoot>/Recordings/` に保存されます）。
5. **Encoding Profile**: 画質とビットレート（4 Mbps / 8 Mbps / 16 Mbps）を選択します。GPU エンコードを使用する場合は `Prefer hardware encoder` を有効にします。

### 2. 録画の実行
1. プラグイン画面の **`RECORD ARM`** ボタンをクリックして点灯（赤色）させます。
   - ※この時点でカメラパイプラインが待機（Prepared）状態になり、録音開始時の遅延をゼロにします。
2. Studio One のトランスポートで **録音（Record）** を開始します。
   - プレビュー画面に `● REC 00:00:00.000` のタイムコードが表示され、自動的に録画が開始されます。
3. Studio One で **停止（Stop）** を押します。
   - 録画が即座に完了・確定し、フッターに直近テイクのファイル名が表示されます。
4. フッターの **`Open Folder`** ボタンを押すと、録画された MP4 ファイルおよび `.sync.json` メタデータが保存されたフォルダがエクスプローラーで開きます。

---

## 動作確認・テスト

実機カメラや IPC 通信が正常に動作するかを単体でテストするためのスモークテストツールが付属しています。

```powershell
# スモークテストの実行
.\build\tools\ipc-smoke\DAWVideoRecorderIpcSmoke_artefacts\Release\DAWVideoRecorderIpcSmoke.exe
```

- 接続されているカメラが検出され、Primary および Shared クライアントによる並行録画テスト、連続テイク録画テスト、同期メタデータの整合性検証が自動実行されます。

---

## ライセンス

本プロジェクトは **GNU Affero General Public License v3.0 (AGPLv3)** の下で公開されています。
詳細は [LICENSE](file:///C:/Users/hiro2/Documents/repo/daw-video-recorder/LICENSE) および [THIRD_PARTY_NOTICES.md](file:///C:/Users/hiro2/Documents/repo/daw-video-recorder/THIRD_PARTY_NOTICES.md) をご参照ください。
