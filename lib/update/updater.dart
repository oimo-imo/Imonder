import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:http/http.dart' as http;
import 'package:open_filex/open_filex.dart';
import 'package:package_info_plus/package_info_plus.dart';
import 'package:path_provider/path_provider.dart';

const githubRepo = 'oimo-imo/Imonder';

class ReleaseInfo {
  const ReleaseInfo({
    required this.version,
    required this.notes,
    required this.assetName,
    required this.assetUrl,
    required this.size,
    this.sha256,
  });

  final String version;
  final String notes;
  final String assetName;
  final Uri assetUrl;
  final int size;
  final String? sha256;
}

/// Parses "v1.2.3", "1.2.3+4" etc. into comparable ints. Returns null if unparsable.
List<int>? parseVersion(String s) {
  final m = RegExp(r'(\d+)\.(\d+)\.(\d+)').firstMatch(s);
  if (m == null) return null;
  return [for (var i = 1; i <= 3; i++) int.parse(m.group(i)!)];
}

bool isNewer(String remote, String local) {
  final r = parseVersion(remote), l = parseVersion(local);
  if (r == null || l == null) return false;
  for (var i = 0; i < 3; i++) {
    if (r[i] != l[i]) return r[i] > l[i];
  }
  return false;
}

/// Picks the release asset for this platform. Android: `*android*.apk`, Windows: `*windows*.zip`.
Map<String, dynamic>? pickAsset(List<dynamic> assets, {required bool android, required bool windows}) {
  final ext = android ? '.apk' : (windows ? '.zip' : null);
  final key = android ? 'android' : 'windows';
  if (ext == null) return null;
  for (final a in assets) {
    final name = (a['name'] as String).toLowerCase();
    if (name.endsWith(ext) && name.contains(key)) return a as Map<String, dynamic>;
  }
  return null;
}

class Updater {
  Updater({http.Client? client}) : _client = client ?? http.Client();
  final http.Client _client;

  bool get supported => Platform.isAndroid || Platform.isWindows;

  Future<String> currentVersion() async => (await PackageInfo.fromPlatform()).version;

  /// Returns the newest release if it is newer than the running app, else null.
  Future<ReleaseInfo?> checkForUpdate() async {
    if (!supported) return null;
    final res = await _client.get(
      Uri.https('api.github.com', '/repos/$githubRepo/releases/latest'),
      headers: {'Accept': 'application/vnd.github+json'},
    ).timeout(const Duration(seconds: 15));
    if (res.statusCode == 404) return null; // no release published yet
    if (res.statusCode != 200) {
      throw HttpException('GitHub API ${res.statusCode}');
    }
    final json = jsonDecode(utf8.decode(res.bodyBytes)) as Map<String, dynamic>;
    final tag = json['tag_name'] as String? ?? '';
    if (!isNewer(tag, await currentVersion())) return null;
    final asset = pickAsset(
      (json['assets'] as List?) ?? const [],
      android: Platform.isAndroid,
      windows: Platform.isWindows,
    );
    if (asset == null) return null;
    final digest = asset['digest'] as String?;
    return ReleaseInfo(
      version: tag.replaceFirst(RegExp(r'^v'), ''),
      notes: (json['body'] as String?) ?? '',
      assetName: asset['name'] as String,
      assetUrl: Uri.parse(asset['browser_download_url'] as String),
      size: (asset['size'] as num?)?.toInt() ?? 0,
      sha256: digest != null && digest.startsWith('sha256:') ? digest.substring(7) : null,
    );
  }

  /// Downloads the asset to a temp file, reporting progress 0..1.
  Future<File> download(ReleaseInfo r, void Function(double) onProgress) async {
    final dir = await getTemporaryDirectory();
    final file = File('${dir.path}${Platform.pathSeparator}${r.assetName}');
    if (await file.exists()) await file.delete();
    final req = http.Request('GET', r.assetUrl);
    final res = await _client.send(req);
    if (res.statusCode != 200) throw HttpException('download failed: ${res.statusCode}');
    final total = res.contentLength ?? r.size;
    final sink = file.openWrite();
    var got = 0;
    try {
      await for (final chunk in res.stream) {
        sink.add(chunk);
        got += chunk.length;
        if (total > 0) onProgress((got / total).clamp(0.0, 1.0));
      }
    } finally {
      await sink.close();
    }
    if (r.sha256 != null) {
      final actual = (await sha256.bind(file.openRead()).first).toString();
      if (actual != r.sha256) {
        await file.delete();
        throw const FormatException('checksum mismatch');
      }
    }
    return file;
  }

  /// Android: hands the APK to the system installer.
  /// Windows: swaps files in place via a helper script, then restarts the app.
  Future<void> install(File file) async {
    if (Platform.isAndroid) {
      final r = await OpenFilex.open(file.path, type: 'application/vnd.android.package-archive');
      if (r.type != ResultType.done) throw StateError('installer: ${r.message}');
    } else if (Platform.isWindows) {
      final exe = File(Platform.resolvedExecutable);
      final appDir = exe.parent.path;
      final tmp = await getTemporaryDirectory();
      final stage = '${tmp.path}\\imonder_update';
      final bat = File('${tmp.path}\\imonder_update.bat');
      await bat.writeAsString([
        '@echo off',
        'setlocal',
        'timeout /t 2 /nobreak >nul',
        'rmdir /s /q "$stage" 2>nul',
        'powershell -NoProfile -Command "Expand-Archive -LiteralPath \'${file.path}\' -DestinationPath \'$stage\' -Force"',
        ':retry',
        'robocopy "$stage" "$appDir" /E /R:5 /W:1 >nul',
        'if %ERRORLEVEL% GEQ 8 goto retry',
        'start "" "${exe.path}"',
        'rmdir /s /q "$stage" 2>nul',
        'del "${file.path}" 2>nul',
        '(goto) 2>nul & del "%~f0"',
      ].join('\r\n'));
      await Process.start('cmd', ['/c', 'start', '', '/min', bat.path], mode: ProcessStartMode.detached);
      exit(0);
    } else {
      throw UnsupportedError('updates are not supported on this platform');
    }
  }
}
