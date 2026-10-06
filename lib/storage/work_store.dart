import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:path_provider/path_provider.dart';

/// A saved work as shown in the gallery.
class WorkInfo {
  const WorkInfo({required this.id, required this.name, required this.updated, this.thumbnail});
  final String id;
  final String name;
  final DateTime updated;
  final File? thumbnail;
}

/// Works live in one folder: `<id>.imnd` (the model), `<id>.png` (thumbnail), `<id>.json` (name, date).
/// Everything is written to a temp file first and renamed, so a crash never leaves a half-written work.
class WorkStore {
  WorkStore(this.root);
  final Directory root;

  static Future<WorkStore> open() async {
    final docs = await getApplicationDocumentsDirectory();
    final dir = Directory('${docs.path}${Platform.pathSeparator}works');
    await dir.create(recursive: true);
    return WorkStore(dir);
  }

  File _file(String id, String ext) => File('${root.path}${Platform.pathSeparator}$id.$ext');

  Future<void> _atomicWrite(File f, List<int> bytes) async {
    final tmp = File('${f.path}.tmp');
    await tmp.writeAsBytes(bytes, flush: true);
    await tmp.rename(f.path);
  }

  Future<void> _writeMeta(String id, String name) =>
      _atomicWrite(_file(id, 'json'), utf8.encode(jsonEncode({'name': name, 'updated': DateTime.now().millisecondsSinceEpoch})));

  Future<Map<String, dynamic>?> _readMeta(String id) async {
    try {
      final m = jsonDecode(await _file(id, 'json').readAsString());
      return m is Map<String, dynamic> && m['name'] is String && m['updated'] is int ? m : null;
    } catch (_) {
      return null;
    }
  }

  /// Creates an empty entry (the model is written by the first [save]). Returns its id.
  Future<String> create({String? name}) async {
    var n = DateTime.now().millisecondsSinceEpoch;
    while (await _file('w$n', 'json').exists()) {
      n++;
    }
    final id = 'w$n';
    await _writeMeta(id, name ?? await _untitledName());
    return id;
  }

  Future<String> _untitledName() async {
    final names = {for (final w in await list()) w.name};
    if (!names.contains('無題')) return '無題';
    for (var i = 2;; i++) {
      if (!names.contains('無題 $i')) return '無題 $i';
    }
  }

  Future<void> save(String id, Uint8List model, {Uint8List? thumbnailPng}) async {
    final meta = await _readMeta(id);
    await _atomicWrite(_file(id, 'imnd'), model);
    if (thumbnailPng != null) await _atomicWrite(_file(id, 'png'), thumbnailPng);
    await _writeMeta(id, (meta?['name'] as String?) ?? await _untitledName());
  }

  Future<Uint8List?> load(String id) async {
    final f = _file(id, 'imnd');
    return await f.exists() ? f.readAsBytes() : null;
  }

  Future<bool> exists(String id) => _file(id, 'json').exists();

  Future<void> rename(String id, String name) async {
    final clean = name.trim();
    if (clean.isNotEmpty && await exists(id)) await _writeMeta(id, clean);
  }

  Future<void> delete(String id) async {
    for (final ext in ['imnd', 'png', 'json']) {
      final f = _file(id, ext);
      if (await f.exists()) await f.delete();
    }
    final last = _file('last', 'txt');
    if (await last.exists() && (await last.readAsString()).trim() == id) await last.delete();
  }

  /// All works, newest first.
  Future<List<WorkInfo>> list() async {
    final out = <WorkInfo>[];
    await for (final e in root.list()) {
      if (e is! File || !e.path.endsWith('.json')) continue;
      final name = e.uri.pathSegments.last;
      final id = name.substring(0, name.length - 5);
      final meta = await _readMeta(id);
      if (meta == null) continue;
      final png = _file(id, 'png');
      out.add(WorkInfo(
        id: id,
        name: meta['name'] as String,
        updated: DateTime.fromMillisecondsSinceEpoch(meta['updated'] as int),
        thumbnail: await png.exists() ? png : null,
      ));
    }
    out.sort((a, b) => b.updated.compareTo(a.updated));
    return out;
  }

  Future<String?> lastOpened() async {
    final f = _file('last', 'txt');
    if (!await f.exists()) return null;
    final id = (await f.readAsString()).trim();
    return await exists(id) ? id : null;
  }

  Future<void> setLastOpened(String id) => _atomicWrite(_file('last', 'txt'), utf8.encode(id));
}
