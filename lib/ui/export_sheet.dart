import 'dart:typed_data';

import 'package:file_picker/file_picker.dart';
import 'package:flutter/material.dart';

import 'viewport.dart';

/// Where exported files go: the system "save as" (Android: pick a folder or Drive; Windows: a
/// save dialog). Returns where it was saved, or null if the user cancelled.
typedef SaveFile = Future<String?> Function(String fileName, Uint8List bytes);

Future<String?> saveWithPicker(String fileName, Uint8List bytes) =>
    FilePicker.saveFile(dialogTitle: '書き出し', fileName: fileName, bytes: bytes);

/// Characters that are not allowed in file names on Windows / Android.
String safeFileName(String name) {
  final clean = name.replaceAll(RegExp(r'[\\/:*?"<>|\x00-\x1F]'), '_').trim();
  return clean.isEmpty ? 'imonder' : clean;
}

/// Bottom sheet: format (glTF / OBJ) and what to include, then save.
class ExportSheet extends StatefulWidget {
  const ExportSheet({super.key, required this.viewport, required this.baseName, required this.hasSelection, this.save});
  final ModelViewportController viewport;
  final String baseName;
  final bool hasSelection;
  final SaveFile? save;

  @override
  State<ExportSheet> createState() => _ExportSheetState();
}

class _ExportSheetState extends State<ExportSheet> {
  bool _obj = false;
  bool _selectedOnly = false;
  bool _busy = false;
  String? _error;

  Future<void> _export() async {
    final bytes = widget.viewport.exportModel(obj: _obj, selectedOnly: _selectedOnly);
    if (bytes == null) return;
    setState(() {
      _busy = true;
      _error = null;
    });
    try {
      final name = '${safeFileName(widget.baseName)}.${_obj ? 'obj' : 'glb'}';
      final where = await (widget.save ?? saveWithPicker)(name, bytes);
      if (mounted) Navigator.pop(context, where == null ? null : name);
    } catch (e) {
      if (mounted) setState(() => _error = '$e');
    } finally {
      if (mounted) setState(() => _busy = false);
    }
  }

  @override
  Widget build(BuildContext context) {
    final accent = Theme.of(context).colorScheme.primary;
    Widget choice(String title, String sub, bool selected, VoidCallback onTap) => Expanded(
          child: Material(
            color: selected ? accent.withValues(alpha: 0.18) : const Color(0xFF303034),
            borderRadius: BorderRadius.circular(14),
            child: InkWell(
              borderRadius: BorderRadius.circular(14),
              onTap: onTap,
              child: Padding(
                padding: const EdgeInsets.all(12),
                child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                  Text(title, style: TextStyle(fontWeight: FontWeight.w600, color: selected ? accent : null)),
                  const SizedBox(height: 2),
                  Text(sub, style: const TextStyle(fontSize: 11, color: Color(0xFF8E8E95))),
                ]),
              ),
            ),
          ),
        );
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 16, 16, 20),
        child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          const Text('書き出し', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
          const SizedBox(height: 12),
          Row(children: [
            choice('glTF (.glb)', 'Blender・Unity・Webなど。おすすめ', !_obj, () => setState(() => _obj = false)),
            const SizedBox(width: 10),
            choice('OBJ (.obj)', '多くのソフトで読める。四角形の面を保つ', _obj, () => setState(() => _obj = true)),
          ]),
          const SizedBox(height: 10),
          Row(children: [
            choice('表示中すべて', '非表示のものは含まない', !_selectedOnly, () => setState(() => _selectedOnly = false)),
            const SizedBox(width: 10),
            choice('選択中だけ', widget.hasSelection ? '選んでいるオブジェクト' : '（何も選んでいません）', _selectedOnly,
                widget.hasSelection ? () => setState(() => _selectedOnly = true) : () {}),
          ]),
          if (_error != null) ...[
            const SizedBox(height: 8),
            Text('書き出せませんでした: $_error', style: const TextStyle(fontSize: 12, color: Color(0xFFE5534B))),
          ],
          const SizedBox(height: 16),
          SizedBox(
            width: double.infinity,
            height: 48,
            child: FilledButton.icon(
              onPressed: _busy ? null : _export,
              icon: _busy
                  ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                  : const Icon(Icons.ios_share, size: 18),
              label: const Text('保存先を選んで書き出す'),
            ),
          ),
        ]),
      ),
    );
  }
}
