import 'dart:typed_data';

import 'package:flutter/material.dart';

import '../storage/work_session.dart';
import '../storage/work_store.dart';
import '../update/updater.dart';
import 'capture_overlay.dart';
import 'gallery_screen.dart';
import 'presets.dart';
import 'view_gizmo.dart';
import 'viewport.dart';

const _panel = Color(0xFF2A2A2E);
const _bar = Color(0xFF161618);
const _muted = Color(0xFF8E8E95);

// Status bits reported by the core (see Core::status in rust/src/lib.rs).
const _bitEditMode = 64;
const _bitHasActive = 128;
const _bitOrtho = 256;
const _bitMany = 2048;
const _bitGrouped = 4096;

class EditorScreen extends StatefulWidget {
  const EditorScreen({super.key, this.saveCapture});

  /// Where captured images go (tests); the photo gallery when null.
  final Future<void> Function(Uint8List png, String name)? saveCapture;

  @override
  State<EditorScreen> createState() => _EditorScreenState();
}

class _EditorScreenState extends State<EditorScreen> with WidgetsBindingObserver {
  final _viewport = ModelViewportController();
  WorkStore? _store;
  WorkSession? _session;
  WorkStore? _presets;
  bool _snap = false;
  int _tool = 0; // 0 move, 1 rotate, 2 scale, 3 loop cut
  ReleaseInfo? _pendingUpdate;
  bool _capturing = false;

  /// Toolbar entries. `tool` >= 0 selects a tool with handles / tap behaviour, `op` >= 0 runs an
  /// operation on the selection right away, `all` selects everything. `kind` picks the action
  /// for the object-mode buttons.
  static const _editTools = <_ToolDef>[
    _ToolDef(Icons.open_with, '移動', tool: 0),
    _ToolDef(Icons.rotate_right, '回転', tool: 1),
    _ToolDef(Icons.zoom_out_map, '拡大縮小', tool: 2),
    _ToolDef(Icons.publish, '押し出し', op: 0, need: '面か辺を選んでください'),
    _ToolDef(Icons.crop_square, 'インセット', op: 1, need: '面を選んでください'),
    _ToolDef(Icons.splitscreen, 'ループカット', tool: 3),
    _ToolDef(Icons.change_history, 'ベベル', op: 3, need: '辺を選んでください'),
    _ToolDef(Icons.call_merge, '結合', op: 4, need: '2つ以上の頂点・辺・面を選んでください'),
    _ToolDef(Icons.delete_outline, '削除', op: 5, need: '先に選択してください'),
    _ToolDef(Icons.library_add_check_outlined, '追加選択', object: _ObjectAction.addSelect),
    _ToolDef(Icons.select_all, '全選択', all: true),
  ];

  static const _objectTools = <_ToolDef>[
    _ToolDef(Icons.add_box_outlined, '追加', object: _ObjectAction.add),
    _ToolDef(Icons.library_add_check_outlined, '追加選択', object: _ObjectAction.addSelect),
    _ToolDef(Icons.select_all, '全選択', object: _ObjectAction.selectAll),
    _ToolDef(Icons.workspaces_outline, 'グループ化', object: _ObjectAction.group),
    _ToolDef(Icons.open_with, '移動', tool: 0),
    _ToolDef(Icons.rotate_right, '回転', tool: 1),
    _ToolDef(Icons.zoom_out_map, '拡大縮小', tool: 2),
    _ToolDef(Icons.copy_all_outlined, '複製', object: _ObjectAction.duplicate),
    _ToolDef(Icons.delete_outline, '削除', object: _ObjectAction.delete),
  ];

  static const _primitives = <(IconData, String)>[
    (Icons.view_in_ar, '立方体'),
    (Icons.crop_landscape, '平面'),
    (Icons.circle_outlined, '円柱'),
    (Icons.change_history, '円錐'),
    (Icons.sports_basketball_outlined, '球'),
    (Icons.donut_large, 'トーラス'),
  ];

  @override
  void initState() {
    super.initState();
    WidgetsBinding.instance.addObserver(this);
    _viewport.setTool(0);
    _backgroundUpdateCheck();
    WidgetsBinding.instance.addPostFrameCallback((_) => _startSession());
  }

  Future<void> _startSession() async {
    final store = await WorkStore.open();
    final presets = await WorkStore.open(folder: 'presets');
    final session = WorkSession(store, _viewport);
    await session.start();
    if (!mounted) return;
    setState(() {
      _store = store;
      _presets = presets;
      _session = session;
    });
  }

  @override
  void dispose() {
    WidgetsBinding.instance.removeObserver(this);
    _session?.flush();
    _session?.dispose();
    super.dispose();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state != AppLifecycleState.resumed) _session?.flush();
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

  /// A short message floating above the bottom toolbar (never covering it).
  void _toast(String text) {
    ScaffoldMessenger.of(context)
      ..hideCurrentSnackBar()
      ..showSnackBar(SnackBar(
        content: Text(text),
        duration: const Duration(seconds: 2),
        behavior: SnackBarBehavior.floating,
        margin: EdgeInsets.fromLTRB(16, 0, 16, MediaQuery.paddingOf(context).bottom + 84),
      ));
  }

  // ------------------------------------------------------------------- gallery --

  Future<void> _openGallery() async {
    final session = _session, store = _store;
    if (session == null || store == null) return;
    await session.flush();
    if (!mounted) return;
    final choice = await Navigator.of(context).push<GalleryChoice>(
      MaterialPageRoute(
        builder: (_) => GalleryScreen(
          store: store,
          currentId: session.id,
          updateAvailable: _pendingUpdate != null,
          presets: _presets,
        ),
      ),
    );
    switch (choice) {
      case OpenWork(:final id):
        if (!await session.open(id)) _toast('この作品を開けませんでした');
      case NewWork(:final presetId):
        final from = presetId == null ? null : await _presets?.load(presetId);
        await session.createNew(from: from);
      case CurrentDeleted():
        await session.createNew();
      case null:
        break;
    }
  }

  // ------------------------------------------------------------------- objects --

  void _showAddSheet() {
    final presets = _presets;
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      backgroundColor: const Color(0xFF252528),
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(26))),
      builder: (ctx) => SafeArea(
        child: Padding(
          padding: const EdgeInsets.fromLTRB(16, 16, 16, 20),
          child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
            GridView.count(
              shrinkWrap: true,
              physics: const NeverScrollableScrollPhysics(),
              crossAxisCount: 3,
              mainAxisSpacing: 10,
              crossAxisSpacing: 10,
              childAspectRatio: 1.35,
              children: [
                for (var i = 0; i < _primitives.length; i++)
                  Material(
                    color: const Color(0xFF303034),
                    borderRadius: BorderRadius.circular(16),
                    child: InkWell(
                      borderRadius: BorderRadius.circular(16),
                      onTap: () {
                        Navigator.pop(ctx);
                        _viewport.addObject(i);
                      },
                      child: Column(mainAxisAlignment: MainAxisAlignment.center, children: [
                        Icon(_primitives[i].$1, color: const Color(0xFFD4D4D9)),
                        const SizedBox(height: 6),
                        Text(_primitives[i].$2, style: const TextStyle(fontSize: 13)),
                      ]),
                    ),
                  ),
              ],
            ),
            if (presets != null) ...[
              const SizedBox(height: 18),
              const Text('プリセット', style: TextStyle(fontSize: 13, color: _muted)),
              const SizedBox(height: 8),
              PresetStrip(
                store: presets,
                onPick: (w) async {
                  Navigator.pop(ctx);
                  final bytes = await presets.load(w.id);
                  if (bytes == null || !_viewport.insertPreset(bytes)) {
                    _toast('プリセットを読み込めませんでした');
                  }
                },
              ),
            ],
          ]),
        ),
      ),
    );
  }

  /// Saves the visible objects as a preset.
  Future<void> _savePreset() async {
    final presets = _presets;
    final bytes = _viewport.presetBytes();
    if (presets == null || bytes == null) return;
    if (!_viewport.objects().any((o) => o.visible)) {
      _toast('表示中のオブジェクトがありません');
      return;
    }
    final count = (await presets.list()).length;
    if (!mounted) return;
    final name = await askName(context, title: '表示中のオブジェクトをプリセットに登録', initial: 'プリセット ${count + 1}');
    if (name == null) return;
    final thumb = await _viewport.thumbnailPng();
    final id = await presets.create(name: name);
    await presets.save(id, bytes, thumbnailPng: thumb);
    _toast('「$name」をプリセットに登録しました');
  }

  void _showOutliner() {
    showModalBottomSheet<void>(
      context: context,
      isScrollControlled: true,
      backgroundColor: const Color(0xFF252528),
      shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(26))),
      builder: (ctx) => StatefulBuilder(builder: (ctx, setSheet) {
        final objects = _viewport.objects();
        return SafeArea(
          child: ConstrainedBox(
            constraints: BoxConstraints(maxHeight: MediaQuery.sizeOf(ctx).height * 0.6),
            child: Column(mainAxisSize: MainAxisSize.min, children: [
              const SizedBox(height: 14),
              Row(children: [
                const SizedBox(width: 20),
                const Expanded(child: Text('シーン', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600))),
                IconButton(
                  tooltip: 'プリセットに登録',
                  icon: const Icon(Icons.bookmark_add_outlined),
                  onPressed: () {
                    Navigator.pop(ctx);
                    _savePreset();
                  },
                ),
                IconButton(
                  tooltip: 'オブジェクトを追加',
                  icon: const Icon(Icons.add),
                  onPressed: () {
                    Navigator.pop(ctx);
                    _showAddSheet();
                  },
                ),
                const SizedBox(width: 8),
              ]),
              if (objects.isEmpty)
                const Padding(padding: EdgeInsets.all(24), child: Text('オブジェクトがありません', style: TextStyle(color: _muted)))
              else
                Flexible(
                  child: ListView.builder(
                    shrinkWrap: true,
                    itemCount: objects.length,
                    itemBuilder: (_, i) {
                      final o = objects[i];
                      return ListTile(
                        selected: o.selected,
                        selectedTileColor: const Color(0xFF303034),
                        // Checkbox: add to / take from the selection. Row: select just this one.
                        leading: Checkbox(
                          value: o.selected,
                          onChanged: (_) {
                            _viewport.toggleObject(i);
                            setSheet(() {});
                          },
                        ),
                        title: Text(o.name),
                        subtitle: o.group == 0
                            ? null
                            : Text('グループ ${o.group}', style: const TextStyle(fontSize: 11, color: _muted)),
                        onTap: () {
                          _viewport.selectObject(i);
                          setSheet(() {});
                        },
                        trailing: IconButton(
                          tooltip: o.visible ? '非表示にする' : '表示する',
                          icon: Icon(o.visible ? Icons.visibility_outlined : Icons.visibility_off_outlined,
                              color: o.visible ? const Color(0xFFD4D4D9) : const Color(0xFF5E5E65)),
                          onPressed: () {
                            _viewport.setObjectVisible(i, !o.visible);
                            setSheet(() {});
                          },
                        ),
                      );
                    },
                  ),
                ),
              const SizedBox(height: 8),
            ]),
          ),
        );
      }),
    );
  }

  // --------------------------------------------------------------------- tools --

  void _useTool(_ToolDef t, int status) {
    if (t.object != null) {
      switch (t.object!) {
        case _ObjectAction.add:
          _showAddSheet();
        case _ObjectAction.duplicate:
          _hasSelectedObject() ? _viewport.duplicateObject() : _toast('オブジェクトを選んでください');
        case _ObjectAction.delete:
          _hasSelectedObject() ? _viewport.deleteObject() : _toast('オブジェクトを選んでください');
        case _ObjectAction.addSelect:
          setState(() => _viewport.addSelect = !_viewport.addSelect);
          _toast(_viewport.addSelect ? '追加選択：タップで選択に足す／外す' : '追加選択をオフにしました');
        case _ObjectAction.selectAll:
          _viewport.selectAllObjects();
        case _ObjectAction.group:
          if (status & _bitGrouped != 0) {
            _viewport.groupObjects(false);
            _toast('グループを解除しました');
          } else if (status & _bitMany != 0) {
            _viewport.groupObjects(true);
            _toast('グループにしました（どれかをタップすると全体が選ばれます）');
          } else {
            _toast('2つ以上のオブジェクトを選んでください（追加選択で選べます）');
          }
      }
    } else if (t.all) {
      _viewport.selectAll();
    } else if (t.tool >= 0) {
      setState(() => _tool = t.tool);
      _viewport.setTool(t.tool);
      if (t.tool == 3) _viewport.setSelectMode(1);
    } else if (!_viewport.runOp(t.op)) {
      _toast(t.need);
    }
  }

  bool _hasSelectedObject() => _viewport.objects().any((o) => o.selected);

  void _toggleMode(int status) {
    if (status & _bitEditMode == 0 && status & _bitHasActive == 0) {
      _toast('オブジェクトを選んでください');
      return;
    }
    final entering = status & _bitEditMode == 0;
    if (!entering && _tool == 3) {
      setState(() => _tool = 0); // the loop cut tool only exists in edit mode
      _viewport.setTool(0);
    }
    _viewport.setEditMode(entering);
  }

  // ------------------------------------------------------------------------ UI --

  @override
  Widget build(BuildContext context) {
    final accent = Theme.of(context).colorScheme.primary;
    final top = MediaQuery.paddingOf(context).top;
    final bottom = MediaQuery.paddingOf(context).bottom;
    return Scaffold(
      backgroundColor: const Color(0xFF1E1E21),
      body: Stack(children: [
        Positioned.fill(child: ModelViewport(controller: _viewport)),
        Positioned.fill(
          child: ValueListenableBuilder<int>(
            valueListenable: _viewport.status,
            builder: (context, status, _) {
              if (_capturing) {
                return CaptureOverlay(
                  viewport: _viewport,
                  saveImage: widget.saveCapture,
                  onClose: () => setState(() => _capturing = false),
                );
              }
              final editMode = status & _bitEditMode != 0;
              final ortho = status & _bitOrtho != 0;
              return Stack(children: [
                Positioned(top: 0, left: 0, right: 0, child: _topBar(accent, status)),
                Positioned(
                  right: 6,
                  top: top + 58,
                  child: ViewGizmo(
                    angles: _viewport.angles,
                    onAxis: _viewport.snapAxis,
                    onReset: () => _viewport.snapView(3),
                    onOrbit: _viewport.orbit,
                  ),
                ),
                Positioned(
                  right: 40,
                  top: top + 58 + 116,
                  child: Tooltip(
                    message: ortho ? '透視投影にする' : '平行投影にする',
                    child: SizedBox(
                      width: 44,
                      height: 44,
                      child: Material(
                        color: ortho ? accent.withValues(alpha: 0.22) : _panel,
                        borderRadius: BorderRadius.circular(14),
                        child: InkWell(
                          borderRadius: BorderRadius.circular(14),
                          onTap: () => _viewport.setOrtho(!ortho),
                          child: Center(
                            child: Text(ortho ? '平行' : '透視',
                                style: TextStyle(fontSize: 13, color: ortho ? accent : const Color(0xFFD4D4D9))),
                          ),
                        ),
                      ),
                    ),
                  ),
                ),
                if (editMode && _tool == 3)
                  Positioned(top: top + 62, left: 16, child: const Text('辺をタップして分割', style: TextStyle(color: _muted, fontSize: 13))),
                if (!editMode && status & _bitHasActive == 0)
                  Positioned(top: top + 62, left: 16, child: const Text('タップでオブジェクトを選択', style: TextStyle(color: _muted, fontSize: 13))),
                Positioned(
                  left: 12,
                  right: 12,
                  bottom: bottom + 84,
                  child: ValueListenableBuilder<(double, double, double, bool)?>(
                    valueListenable: _viewport.opRange,
                    builder: (context, r, _) => r == null ? const SizedBox.shrink() : _adjustCapsule(accent, r),
                  ),
                ),
                Positioned(left: 12, right: 12, bottom: bottom + 20, child: _toolBar(accent, status)),
              ]);
            },
          ),
        ),
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
                child: Container(width: 8, height: 8, decoration: const BoxDecoration(color: Color(0xFFF2A35E), shape: BoxShape.circle)),
              ),
          ]),
        ),
      ),
    );
  }

  Widget _topBar(Color accent, int status) {
    final editMode = status & _bitEditMode != 0;
    final selectMode = (status >> 4) & 3;
    const selIcons = [Icons.scatter_plot, Icons.show_chart, Icons.crop_square];
    const selLabels = ['頂点', '辺', '面'];
    return Container(
      color: _bar,
      padding: EdgeInsets.fromLTRB(6, MediaQuery.paddingOf(context).top, 6, 0),
      height: 52 + MediaQuery.paddingOf(context).top,
      child: Row(children: [
        _iconBtn(Icons.grid_view, _pendingUpdate != null ? 'ギャラリー（アップデートがあります）' : 'ギャラリー',
            _session == null ? null : _openGallery, dot: _pendingUpdate != null),
        _iconBtn(Icons.photo_camera_outlined, '撮影', () => setState(() => _capturing = true), color: const Color(0xFFD4D4D9)),
        Semantics(
          button: true,
          toggled: _snap,
          label: 'スナップ',
          child: SizedBox(
            width: 44,
            height: 44,
            child: IconButton(
              tooltip: _snap ? 'スナップ：オン' : 'スナップ：オフ',
              padding: EdgeInsets.zero,
              style: _snap ? IconButton.styleFrom(backgroundColor: accent.withValues(alpha: 0.2)) : null,
              onPressed: () {
                setState(() => _snap = !_snap);
                _viewport.setSnap(_snap);
                _toast(_snap ? 'スナップ：面・グリッドに吸着／回転15°／拡大0.1倍刻み' : 'スナップをオフにしました');
              },
              icon: MagnetIcon(color: _snap ? accent : _muted),
            ),
          ),
        ),
        const Spacer(),
        _iconBtn(Icons.layers_outlined, 'シーン', _showOutliner, color: _muted),
        _iconBtn(Icons.hub_outlined, editMode ? '編集モード' : 'オブジェクトモード', () => _toggleMode(status), color: editMode ? accent : _muted),
        Container(width: 1, height: 20, color: const Color(0xFF333338), margin: const EdgeInsets.symmetric(horizontal: 4)),
        for (var i = 0; i < 3; i++)
          _iconBtn(selIcons[i], selLabels[i], editMode ? () => _viewport.setSelectMode(i) : null,
              color: !editMode ? const Color(0xFF4A4A50) : (selectMode == i ? const Color(0xFFE6E6E9) : _muted)),
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

  Widget _toolBar(Color accent, int status) {
    final editMode = status & _bitEditMode != 0;
    final tools = editMode ? _editTools : _objectTools;
    return Row(children: [
      Expanded(
        child: Container(
          padding: const EdgeInsets.all(4),
          decoration: BoxDecoration(color: _panel, borderRadius: BorderRadius.circular(26)),
          child: SingleChildScrollView(
            scrollDirection: Axis.horizontal,
            child: Row(children: [
              for (final t in tools)
                Builder(builder: (context) {
                  final on = (t.tool >= 0 && t.tool == _tool) || (t.object == _ObjectAction.addSelect && _viewport.addSelect);
                  final ungroup = t.object == _ObjectAction.group && status & _bitGrouped != 0;
                  return SizedBox(
                    width: 44,
                    height: 44,
                    child: IconButton(
                      tooltip: ungroup ? 'グループ解除' : t.label,
                      padding: EdgeInsets.zero,
                      style: on ? IconButton.styleFrom(backgroundColor: accent, foregroundColor: const Color(0xFF1A1A1C)) : null,
                      onPressed: () => _useTool(t, status),
                      icon: Icon(ungroup ? Icons.workspaces_filled : t.icon,
                          size: 20, color: on ? const Color(0xFF1A1A1C) : const Color(0xFFA9A9B0)),
                    ),
                  );
                }),
            ]),
          ),
        ),
      ),
      const SizedBox(width: 8),
      Container(
        width: 52,
        height: 52,
        decoration: const BoxDecoration(color: _panel, shape: BoxShape.circle),
        child: Builder(builder: (context) {
          final can = status & 1 != 0;
          return IconButton(
            tooltip: '元に戻す',
            onPressed: can ? _viewport.undo : null,
            icon: Icon(Icons.undo, size: 19, color: can ? const Color(0xFFE6E6E9) : const Color(0xFF5E5E65)),
          );
        }),
      ),
    ]);
  }
}

enum _ObjectAction { add, duplicate, delete, addSelect, selectAll, group }

class _ToolDef {
  const _ToolDef(this.icon, this.label, {this.tool = -1, this.op = -1, this.all = false, this.need = '', this.object});
  final IconData icon;
  final String label;
  final int tool;
  final int op;
  final bool all;
  final String need;
  final _ObjectAction? object;
}
