import 'dart:async';

import '../ui/viewport.dart';
import 'work_store.dart';

/// Ties the open work to its file: autosaves when it changes and switches between works.
class WorkSession {
  WorkSession(this.store, this.viewport);

  final WorkStore store;
  final ModelViewportController viewport;

  String? id;
  int _savedRevision = -1;
  bool _saving = false;
  Timer? _timer;

  /// Opens the work that was open last time, or starts a new one.
  Future<void> start() async {
    final last = await store.lastOpened();
    if (last == null || !await _open(last)) await createNew();
    _timer = Timer.periodic(const Duration(seconds: 2), (_) => saveIfChanged());
  }

  void dispose() => _timer?.cancel();

  Future<void> saveIfChanged() => _save(force: false);

  Future<void> _save({required bool force}) async {
    final wid = id;
    if (wid == null || _saving) return;
    final rev = viewport.revision;
    if (!force && rev == _savedRevision) return;
    final bytes = viewport.saveBytes();
    if (bytes == null) return;
    _saving = true;
    try {
      final thumb = await viewport.thumbnailPng();
      await store.save(wid, bytes, thumbnailPng: thumb);
      _savedRevision = rev;
    } finally {
      _saving = false;
    }
  }

  /// Saves the current work (call before leaving the screen or the app).
  Future<void> flush() async {
    while (_saving) {
      await Future<void>.delayed(const Duration(milliseconds: 20));
    }
    await _save(force: false);
  }

  Future<bool> _open(String wid) async {
    final bytes = await store.load(wid);
    if (bytes == null) {
      // Created but never saved: start from the default scene.
      viewport.newWork();
    } else if (!viewport.loadBytes(bytes)) {
      return false;
    }
    id = wid;
    _savedRevision = viewport.revision;
    await store.setLastOpened(wid);
    if (bytes == null) await _save(force: true);
    return true;
  }

  Future<bool> open(String wid) async {
    if (wid == id) return true;
    await flush();
    final previous = id;
    if (await _open(wid)) return true;
    id = previous;
    return false;
  }

  Future<void> createNew() async {
    await flush();
    final wid = await store.create();
    viewport.newWork();
    id = wid;
    _savedRevision = viewport.revision;
    await store.setLastOpened(wid);
    await _save(force: true);
  }
}
