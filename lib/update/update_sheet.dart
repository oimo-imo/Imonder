import 'package:flutter/material.dart';

import 'updater.dart';

enum _Phase { idle, checking, upToDate, available, downloading, installing, error }

/// Bottom sheet: shows the version, checks GitHub Releases and installs updates.
class UpdateSheet extends StatefulWidget {
  const UpdateSheet({super.key, this.autoCheck = true, this.updater});
  final bool autoCheck;
  final Updater? updater;

  @override
  State<UpdateSheet> createState() => _UpdateSheetState();
}

class _UpdateSheetState extends State<UpdateSheet> {
  late final Updater _updater = widget.updater ?? Updater();
  _Phase _phase = _Phase.idle;
  String _current = '';
  ReleaseInfo? _release;
  double _progress = 0;
  String _error = '';

  @override
  void initState() {
    super.initState();
    _updater.currentVersion().then((v) {
      if (mounted) setState(() => _current = v);
    });
    if (widget.autoCheck) _check();
  }

  Future<void> _check() async {
    setState(() => _phase = _Phase.checking);
    try {
      final r = await _updater.checkForUpdate();
      if (!mounted) return;
      setState(() {
        _release = r;
        _phase = r == null ? _Phase.upToDate : _Phase.available;
      });
    } catch (e) {
      _fail(e);
    }
  }

  Future<void> _install() async {
    final r = _release!;
    setState(() {
      _phase = _Phase.downloading;
      _progress = 0;
    });
    try {
      final file = await _updater.download(r, (p) {
        if (mounted) setState(() => _progress = p);
      });
      if (!mounted) return;
      setState(() => _phase = _Phase.installing);
      await _updater.install(file);
    } catch (e) {
      _fail(e);
    }
  }

  void _fail(Object e) {
    if (!mounted) return;
    setState(() {
      _phase = _Phase.error;
      _error = '$e';
    });
  }

  @override
  Widget build(BuildContext context) {
    final t = Theme.of(context).textTheme;
    final accent = Theme.of(context).colorScheme.primary;
    final busy = _phase == _Phase.checking || _phase == _Phase.downloading || _phase == _Phase.installing;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(20, 8, 20, 24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Center(
              child: Container(
                width: 36,
                height: 4,
                decoration: BoxDecoration(color: const Color(0xFF4A4A50), borderRadius: BorderRadius.circular(2)),
              ),
            ),
            const SizedBox(height: 16),
            Text('Imonder', style: t.titleMedium),
            const SizedBox(height: 4),
            Text('バージョン $_current',
                style: t.bodySmall?.copyWith(fontFamily: 'monospace', color: const Color(0xFF8E8E95))),
            const SizedBox(height: 16),
            if (!_updater.supported)
              Text('このプラットフォームでは自動更新に対応していません', style: t.bodyMedium)
            else ...[
              Text(_message(), style: t.bodyMedium),
              if (_phase == _Phase.available && (_release?.notes.trim().isNotEmpty ?? false)) ...[
                const SizedBox(height: 8),
                ConstrainedBox(
                  constraints: const BoxConstraints(maxHeight: 140),
                  child: SingleChildScrollView(
                    child: Text(_release!.notes, style: t.bodySmall?.copyWith(color: const Color(0xFFA9A9B0))),
                  ),
                ),
              ],
              if (_phase == _Phase.downloading) ...[
                const SizedBox(height: 12),
                LinearProgressIndicator(value: _progress, color: accent, backgroundColor: const Color(0xFF3C3C42)),
              ],
              if (_phase == _Phase.error) ...[
                const SizedBox(height: 4),
                Text(_error, style: t.bodySmall?.copyWith(color: const Color(0xFFE5534B))),
              ],
              const SizedBox(height: 16),
              SizedBox(
                width: double.infinity,
                height: 48,
                child: FilledButton(
                  onPressed: busy ? null : (_phase == _Phase.available ? _install : _check),
                  child: Text(_phase == _Phase.available ? 'アップデートしてインストール' : 'アップデートを確認'),
                ),
              ),
            ],
          ],
        ),
      ),
    );
  }

  String _message() => switch (_phase) {
        _Phase.idle => '',
        _Phase.checking => '確認中…',
        _Phase.upToDate => '最新です',
        _Phase.available => '新しいバージョン ${_release!.version} があります（${(_release!.size / 1048576).toStringAsFixed(1)} MB）',
        _Phase.downloading => 'ダウンロード中… ${(_progress * 100).round()}%',
        _Phase.installing => 'インストールしています…',
        _Phase.error => '更新に失敗しました',
      };
}
