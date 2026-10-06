# Imonder

指だけで作れる 3D モデリングアプリ（Android / Windows）。仕様は `3Dモデリングツール 仕様書` を参照。

- UI: Flutter (`lib/`)
- コア・描画: Rust (`rust/`)。C ABI を `dart:ffi` で呼ぶ。現状は CPU ラスタライザ（`render.rs`）で、wgpu への差し替えを想定した境界になっている
- 編集モード: 頂点・辺・面をタップで選択（マウスは Shift+クリックで追加選択）、移動ツールで矢印をドラッグして軸方向に移動、2本指タップで元に戻す・3本指タップでやり直す
- 視点: 右上のギズモの軸をタップでその方向へ、ドラッグで回転、背景タップでパースに戻る
- 操作: タッチ = 1本指で回転 / 2本指で移動＋ピンチズーム。マウス = ドラッグで回転、Shift＋ドラッグ or 右ドラッグで移動、ホイールでズーム

## 開発

```sh
cargo test --release --manifest-path rust/Cargo.toml
cargo build --release --manifest-path rust/Cargo.toml   # Dart側FFIテスト・Windows実行に必要
flutter test && flutter analyze
```

Android は `cargo ndk -t arm64-v8a -o ../android/app/src/main/jniLibs build --release`（`rust/` 内）で `.so` を作ってから `flutter build apk`。

## リリースとアプリ内アップデート

1. GitHub の Actions → **Build & Release** → Run workflow で `version`（例 `0.1.1`）を入れる。または `v0.1.1` タグを push。
2. APK（`imonder-android-arm64.apk`）と Windows zip（`imonder-windows-x64.zip`）が GitHub Releases に上がる。
3. アプリは起動時に最新リリースを確認し、新しければ上バーのアップデートアイコンに印が付く。タップ → ダウンロード → インストール。
   - Android: システムのインストーラが開く（初回は「この提供元のアプリを許可」が必要）
   - Windows: 終了 → ファイル入れ替え → 自動で再起動

### Android の署名について
更新するには前回と同じ鍵で署名する必要がある。既定ではリポジトリ内の開発用キー（`android/imonder-dev.jks`）を使う。
自分だけの鍵に変えるときは、Secrets に `ANDROID_KEYSTORE_BASE64` / `ANDROID_KEYSTORE_PASSWORD` / `ANDROID_KEY_ALIAS` / `ANDROID_KEY_PASSWORD` を登録する（ただし鍵を変えると既存インストールは一度アンインストールが必要）。
