import 'package:flutter/material.dart';

import '../update/update_sheet.dart';
import '../update/updater.dart';
import 'view_gizmo.dart';
import 'viewport.dart';

const _panel = Color(0xFF2A2A2E);
const _bar = Color(0xFF161618);
const _muted = Color(0xFF8E8E95);

class EditorScreen extends StatefulWidget {
  const EditorScreen({super.key});

  @override
  State<EditorScreen> createState() => _EditorScreenState();
}

class _EditorScreenState extends State<EditorScreen> {
  final _viewport = ModelViewportController();
  int _tool = 0;
  int _selectMode = 2; // 0 vertex, 1 edge, 2 face
  bool _editMode = false;
  ReleaseInfo? _pendingUpdate;

  static const _tools = <(IconData, String)>[
    (Icons.open_with, '移動'),
    (Icons.rotate_right, '回転'),
    (Icons.zoom_out_map, '拡大縮小'),
    (Icons.publish, '押し出し'),
    (Icons.crop_square, 'インセット'),
    (Icons.splitscreen, 'ループカット'),
    (Icons.change_history, 'ベベル'),
  ];

  @override
  void initState() {
    super.initState();
    _backgroundUpdateCheck();
  }

  Future<void> _backgroundUpdateCheck() async {
    final u = Updater();
    if (!u.supported) return;
    try {
      final r = await u.checkForUpdate();
      if (mounted && r != null) setState(() => _pendingUpdate = r);
    } catch (_) {
      // Offline or rate limited: stay quiet, the manual check reports errors.
    }
  }

  void _openSettings() {
    showModalBottomSheet<void>(
      context: context,
      backgroundColor: const Color(0xFF252528),
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(26))),
      builder: (_) => const UpdateSheet(),
    );
  }

  @override
  Widget build(BuildContext context) {
    final accent = Theme.of(context).colorScheme.primary;
    return Scaffold(
      backgroundColor: const Color(0xFF1E1E21),
      body: Stack(children: [
        Positioned.fill(child: ModelViewport(controller: _viewport)),
        Positioned(top: 0, left: 0, right: 0, child: _topBar(accent)),
        Positioned(
          right: 6,
          top: MediaQuery.paddingOf(context).top + 58,
          child: ViewGizmo(
            angles: _viewport.angles,
            onAxis: _viewport.snapAxis,
            onReset: () => _viewport.snapView(3),
          ),
        ),
        Positioned(left: 12, right: 12, bottom: MediaQuery.paddingOf(context).bottom + 20, child: _toolBar(accent)),
      ]),
    );
  }

  Widget _iconBtn(IconData icon, String label, VoidCallback? onTap, {Color? color, bool dot = false}) {
    return Semantics(
      label: label,
      button: true,
      child: SizedBox(
        width: 44,
        height: 44,
        child: IconButton(
          tooltip: label,
          padding: EdgeInsets.zero,
          onPressed: onTap,
          icon: Stack(clipBehavior: Clip.none, children: [
            Icon(icon, size: 21, color: color ?? const Color(0xFFE6E6E9)),
            if (dot)
              Positioned(
                right: -3,
                top: -3,
                child: Container(
                    width: 8, height: 8, decoration: const BoxDecoration(color: Color(0xFFF2A35E), shape: BoxShape.circle)),
              ),
          ]),
        ),
      ),
    );
  }

  Widget _topBar(Color accent) {
    final selIcons = [Icons.scatter_plot, Icons.show_chart, Icons.crop_square];
    final selLabels = ['頂点', '辺', '面'];
    return Container(
      color: _bar,
      padding: EdgeInsets.fromLTRB(6, MediaQuery.paddingOf(context).top, 6, 0),
      height: 52 + MediaQuery.paddingOf(context).top,
      child: Row(children: [
        _iconBtn(Icons.grid_view, 'ギャラリー', null, color: _muted),
        _iconBtn(Icons.cloud_done_outlined, '同期済み', null, color: _muted),
        _iconBtn(Icons.system_update_alt, 'アップデート', _openSettings,
            color: _pendingUpdate != null ? accent : _muted, dot: _pendingUpdate != null),
        const Spacer(),
        _iconBtn(Icons.hub_outlined, _editMode ? '編集モード' : 'オブジェクトモード', () => setState(() => _editMode = !_editMode),
            color: _editMode ? accent : _muted),
        Container(width: 1, height: 20, color: const Color(0xFF333338), margin: const EdgeInsets.symmetric(horizontal: 4)),
        for (var i = 0; i < 3; i++)
          _iconBtn(selIcons[i], selLabels[i], _editMode ? () => setState(() => _selectMode = i) : null,
              color: !_editMode ? const Color(0xFF4A4A50) : (_selectMode == i ? const Color(0xFFE6E6E9) : _muted)),
      ]),
    );
  }

  Widget _toolBar(Color accent) {
    return Row(children: [
      Expanded(
        child: Container(
          padding: const EdgeInsets.all(4),
          decoration: BoxDecoration(color: _panel, borderRadius: BorderRadius.circular(26)),
          child: Row(mainAxisAlignment: MainAxisAlignment.spaceBetween, children: [
            for (var i = 0; i < _tools.length; i++)
              SizedBox(
                width: 40,
                height: 44,
                child: IconButton(
                  tooltip: _tools[i].$2,
                  padding: EdgeInsets.zero,
                  style: i == _tool ? IconButton.styleFrom(backgroundColor: accent, foregroundColor: const Color(0xFF1A1A1C)) : null,
                  onPressed: () => setState(() => _tool = i),
                  icon: Icon(_tools[i].$1, size: 20, color: i == _tool ? const Color(0xFF1A1A1C) : const Color(0xFFA9A9B0)),
                ),
              ),
          ]),
        ),
      ),
      const SizedBox(width: 8),
      Container(
        width: 52,
        height: 52,
        decoration: const BoxDecoration(color: _panel, shape: BoxShape.circle),
        child: IconButton(
          tooltip: '元に戻す',
          onPressed: null,
          icon: const Icon(Icons.undo, size: 19, color: Color(0xFF5E5E65)),
        ),
      ),
    ]);
  }
}
