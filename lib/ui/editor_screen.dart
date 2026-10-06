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
  int _tool = 0; // 0 move, 1 rotate, 2 scale, 3 loop cut
  int _selectMode = 2; // 0 vertex, 1 edge, 2 face
  bool _editMode = false;
  ReleaseInfo? _pendingUpdate;

  /// Toolbar entries. `tool` >= 0 selects a tool with handles / tap behaviour,
  /// `op` >= 0 runs an operation on the selection right away, `all` selects everything.
  static const _tools = <_ToolDef>[
    _ToolDef(Icons.open_with, '移動', tool: 0),
    _ToolDef(Icons.rotate_right, '回転', tool: 1),
    _ToolDef(Icons.zoom_out_map, '拡大縮小', tool: 2),
    _ToolDef(Icons.publish, '押し出し', op: 0, need: '面か辺を選んでください'),
    _ToolDef(Icons.crop_square, 'インセット', op: 1, need: '面を選んでください'),
    _ToolDef(Icons.splitscreen, 'ループカット', tool: 3),
    _ToolDef(Icons.change_history, 'ベベル', op: 3, need: '辺を選んでください'),
    _ToolDef(Icons.call_merge, '結合', op: 4, need: '2つ以上の頂点・辺・面を選んでください'),
    _ToolDef(Icons.delete_outline, '削除', op: 5, need: '先に選択してください'),
    _ToolDef(Icons.select_all, '全選択', all: true),
  ];

  @override
  void initState() {
    super.initState();
    _viewport.setTool(0);
    _viewport.status.addListener(_syncSelectMode);
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

  /// The core can change the select mode itself (e.g. merge leaves a vertex selected).
  void _syncSelectMode() {
    final mode = (_viewport.status.value >> 4) & 3;
    if (mode == _selectMode) return;
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (mounted) setState(() => _selectMode = mode);
    });
  }

  void _enterEditMode() {
    if (_editMode) return;
    setState(() => _editMode = true);
    _viewport.setEditMode(true);
  }

  void _useTool(_ToolDef t) {
    _enterEditMode();
    if (t.all) {
      _viewport.selectAll();
    } else if (t.tool >= 0) {
      setState(() => _tool = t.tool);
      _viewport.setTool(t.tool);
      if (t.tool == 3) {
        setState(() => _selectMode = 1);
        _viewport.setSelectMode(1);
      }
    } else if (!_viewport.runOp(t.op)) {
      ScaffoldMessenger.of(context)
        ..hideCurrentSnackBar()
        ..showSnackBar(SnackBar(content: Text(t.need), duration: const Duration(seconds: 2)));
    }
  }

  @override
  void dispose() {
    _viewport.status.removeListener(_syncSelectMode);
    super.dispose();
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
            onOrbit: _viewport.orbit,
          ),
        ),
        if (_editMode && _tool == 3)
          Positioned(
            top: MediaQuery.paddingOf(context).top + 62,
            left: 16,
            child: const Text('辺をタップして分割', style: TextStyle(color: _muted, fontSize: 13)),
          ),
        Positioned(
          left: 12,
          right: 12,
          bottom: MediaQuery.paddingOf(context).bottom + 84,
          child: ValueListenableBuilder<(double, double, double, bool)?>(
            valueListenable: _viewport.opRange,
            builder: (context, r, _) => r == null ? const SizedBox.shrink() : _adjustCapsule(accent, r),
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
        _iconBtn(Icons.hub_outlined, _editMode ? '編集モード' : 'オブジェクトモード', () {
          setState(() => _editMode = !_editMode);
          _viewport.setEditMode(_editMode);
        },
            color: _editMode ? accent : _muted),
        Container(width: 1, height: 20, color: const Color(0xFF333338), margin: const EdgeInsets.symmetric(horizontal: 4)),
        for (var i = 0; i < 3; i++)
          _iconBtn(selIcons[i], selLabels[i], _editMode
              ? () {
                  setState(() => _selectMode = i);
                  _viewport.setSelectMode(i);
                }
              : null,
              color: !_editMode ? const Color(0xFF4A4A50) : (_selectMode == i ? const Color(0xFFE6E6E9) : _muted)),
      ]),
    );
  }

  /// "Adjust last operation" capsule: icon, slider, value.
  Widget _adjustCapsule(Color accent, (double, double, double, bool) r) {
    final (lo, hi, cur, isInt) = r;
    return Container(
      height: 56,
      padding: const EdgeInsets.fromLTRB(6, 0, 16, 0),
      decoration: BoxDecoration(color: _panel, borderRadius: BorderRadius.circular(28)),
      child: Row(children: [
        Container(
          width: 44,
          height: 44,
          decoration: const BoxDecoration(color: Color(0xFF34343A), shape: BoxShape.circle),
          child: Icon(Icons.tune, size: 19, color: accent),
        ),
        Expanded(
          child: SliderTheme(
            data: SliderTheme.of(context).copyWith(activeTrackColor: accent, thumbColor: const Color(0xFFE6E6E9)),
            child: Slider(
              value: cur.clamp(lo, hi),
              min: lo,
              max: hi,
              divisions: isInt ? (hi - lo).round() : null,
              onChanged: _viewport.adjustOp,
            ),
          ),
        ),
        SizedBox(
          width: 44,
          child: Text(
            isInt ? cur.round().toString() : cur.toStringAsFixed(2),
            textAlign: TextAlign.right,
            style: const TextStyle(fontFamily: 'monospace', fontSize: 15, fontWeight: FontWeight.w500),
          ),
        ),
      ]),
    );
  }

  Widget _toolBar(Color accent) {
    return Row(children: [
      Expanded(
        child: Container(
          padding: const EdgeInsets.all(4),
          decoration: BoxDecoration(color: _panel, borderRadius: BorderRadius.circular(26)),
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(children: [
              for (final t in _tools)
                SizedBox(
                  width: 44,
                  height: 44,
                  child: IconButton(
                    tooltip: t.label,
                    padding: EdgeInsets.zero,
                    style: _editMode && t.tool >= 0 && t.tool == _tool
                        ? IconButton.styleFrom(backgroundColor: accent, foregroundColor: const Color(0xFF1A1A1C))
                        : null,
                    onPressed: () => _useTool(t),
                    icon: Icon(t.icon,
                        size: 20,
                        color: _editMode && t.tool >= 0 && t.tool == _tool ? const Color(0xFF1A1A1C) : const Color(0xFFA9A9B0)),
                  ),
                ),
            ]),
          ),
        ),
      ),
      const SizedBox(width: 8),
      Container(
        width: 52,
        height: 52,
        decoration: const BoxDecoration(color: _panel, shape: BoxShape.circle),
        child: ValueListenableBuilder<int>(
          valueListenable: _viewport.status,
          builder: (context, status, _) {
            final can = status & 1 != 0;
            return IconButton(
              tooltip: '元に戻す',
              onPressed: can ? _viewport.undo : null,
              icon: Icon(Icons.undo, size: 19, color: can ? const Color(0xFFE6E6E9) : const Color(0xFF5E5E65)),
            );
          },
        ),
      ),
    ]);
  }
}

class _ToolDef {
  const _ToolDef(this.icon, this.label, {this.tool = -1, this.op = -1, this.all = false, this.need = ''});
  final IconData icon;
  final String label;
  final int tool;
  final int op;
  final bool all;
  final String need;
}
