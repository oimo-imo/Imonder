import 'package:flutter/material.dart';

import '../core/native_core.dart';
import 'viewport.dart';

const _card = Color(0xFF303034);
const _chip = Color(0xFF3A3A40);
const _muted = Color(0xFF8E8E95);
const _axisColors = [Color(0xFFE5534B), Color(0xFF7BC86C), Color(0xFF5B8DEF)];
const _axisNames = ['X', 'Y', 'Z'];

IconData modifierIcon(String kind) => kind == 'mirror' ? Icons.flip : Icons.view_column_outlined;
String modifierName(String kind) => kind == 'mirror' ? 'ミラー' : '配列';

/// The active object's modifier stack (phone layout from the mockup): the stack as icons on
/// top with a "+" to add, the chosen modifier's settings below.
class ModifierSheet extends StatefulWidget {
  const ModifierSheet({super.key, required this.viewport});
  final ModelViewportController viewport;

  @override
  State<ModifierSheet> createState() => _ModifierSheetState();
}

class _ModifierSheetState extends State<ModifierSheet> {
  int _current = 0;

  ModelViewportController get _vp => widget.viewport;

  void _set(int param, double value) {
    _vp.modifierSet(_current, param, value);
    setState(() {});
  }

  Future<void> _add() async {
    final kind = await showModalBottomSheet<int>(
      context: context,
      backgroundColor: const Color(0xFF252528),
      builder: (ctx) => SafeArea(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          ListTile(
            leading: Icon(modifierIcon('mirror')),
            title: const Text('ミラー'),
            subtitle: const Text('原点を通る面で左右対称にする'),
            onTap: () => Navigator.pop(ctx, 0),
          ),
          ListTile(
            leading: Icon(modifierIcon('array')),
            title: const Text('配列'),
            subtitle: const Text('同じ形を等間隔に並べる'),
            onTap: () => Navigator.pop(ctx, 1),
          ),
        ]),
      ),
    );
    if (kind == null) return;
    final i = _vp.modifierAdd(kind);
    if (i >= 0) setState(() => _current = i);
  }

  @override
  Widget build(BuildContext context) {
    final accent = Theme.of(context).colorScheme.primary;
    final mods = _vp.modifiers();
    final hasObject = _vp.objects().any((o) => o.active);
    if (_current >= mods.length) _current = mods.isEmpty ? 0 : mods.length - 1;
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 10, 16, 20),
        child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Center(
            child: Container(width: 36, height: 4, decoration: BoxDecoration(color: const Color(0xFF4A4A50), borderRadius: BorderRadius.circular(2))),
          ),
          const SizedBox(height: 14),
          if (!hasObject)
            const Padding(
              padding: EdgeInsets.symmetric(vertical: 24),
              child: Text('オブジェクトを選んでください', style: TextStyle(color: _muted)),
            )
          else ...[
            Row(children: [
              for (var i = 0; i < mods.length; i++)
                Padding(
                  padding: const EdgeInsets.only(right: 8),
                  child: Tooltip(
                    message: modifierName(mods[i].kind),
                    child: Material(
                      color: i == _current ? const Color(0xFF3A3A40) : _card,
                      shape: RoundedRectangleBorder(
                        borderRadius: BorderRadius.circular(16),
                        side: i == _current ? BorderSide(color: accent, width: 2) : BorderSide.none,
                      ),
                      child: InkWell(
                        borderRadius: BorderRadius.circular(16),
                        onTap: () => setState(() => _current = i),
                        child: SizedBox(
                          width: 52,
                          height: 52,
                          child: Icon(modifierIcon(mods[i].kind),
                              color: mods[i].enabled ? const Color(0xFFD4D4D9) : const Color(0xFF5E5E65)),
                        ),
                      ),
                    ),
                  ),
                ),
              Tooltip(
                message: 'モディファイアを追加',
                child: InkWell(
                  borderRadius: BorderRadius.circular(16),
                  onTap: _add,
                  child: Container(
                    width: 52,
                    height: 52,
                    decoration: BoxDecoration(
                      border: Border.all(color: const Color(0xFF4A4A50), width: 1.5),
                      borderRadius: BorderRadius.circular(16),
                    ),
                    child: const Icon(Icons.add, color: _muted, size: 20),
                  ),
                ),
              ),
            ]),
            const SizedBox(height: 14),
            if (mods.isEmpty)
              const Text('「＋」でミラーか配列を追加します。元の形は残るので、いつでも外せます。', style: TextStyle(fontSize: 12, color: _muted))
            else
              _settings(mods[_current], accent),
          ],
        ]),
      ),
    );
  }

  Widget _settings(ModifierInfo m, Color accent) {
    return Container(
      padding: const EdgeInsets.fromLTRB(14, 6, 6, 14),
      decoration: BoxDecoration(color: _card, borderRadius: BorderRadius.circular(20)),
      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
        Row(children: [
          Expanded(child: Text(modifierName(m.kind), style: const TextStyle(fontSize: 14, fontWeight: FontWeight.w600))),
          IconButton(
            tooltip: m.enabled ? '結果を隠す' : '結果を表示',
            onPressed: () => _set(0, m.enabled ? 0 : 1),
            icon: Icon(m.enabled ? Icons.visibility_outlined : Icons.visibility_off_outlined, size: 20),
          ),
          IconButton(
            tooltip: '適用（形に焼き込む）',
            onPressed: () {
              _vp.modifierFinish(_current, apply: true);
              setState(() {});
            },
            icon: const Icon(Icons.check, size: 20),
          ),
          IconButton(
            tooltip: '削除',
            onPressed: () {
              _vp.modifierFinish(_current, apply: false);
              setState(() {});
            },
            icon: const Icon(Icons.delete_outline, size: 20),
          ),
        ]),
        if (m.kind == 'mirror') ...[
          _labelled('軸', Row(children: [for (var a = 0; a < 3; a++) _axisChip(a, m.axes[a], () => _set(1 + a, m.axes[a] ? 0 : 1))])),
          _labelled('結合', _switch(m.merge, (v) => _set(4, v ? 1 : 0), '鏡の面上の頂点をつなげる')),
        ] else ...[
          _labelled(
            '個数',
            Row(children: [
              _round(Icons.remove, '減らす', m.count > 1 ? () => _set(1, (m.count - 1).toDouble()) : null),
              Expanded(
                child: Text('${m.count}',
                    textAlign: TextAlign.center, style: const TextStyle(fontSize: 19, fontWeight: FontWeight.w500, fontFamily: 'monospace')),
              ),
              _round(Icons.add, '増やす', m.count < 64 ? () => _set(1, (m.count + 1).toDouble()) : null),
            ]),
          ),
          _labelled(
            '間隔',
            Row(children: [
              Expanded(
                child: SliderTheme(
                  data: SliderTheme.of(context).copyWith(activeTrackColor: accent, thumbColor: const Color(0xFFE6E6E9)),
                  child: Slider(value: m.spacing.clamp(-3.0, 3.0), min: -3, max: 3, onChanged: (v) => _set(2, (v * 20).round() / 20)),
                ),
              ),
              SizedBox(
                width: 56,
                child: Text(m.spacing.toStringAsFixed(2),
                    maxLines: 1, softWrap: false, textAlign: TextAlign.right, style: const TextStyle(fontFamily: 'monospace')),
              ),
            ]),
          ),
          _labelled('方向', Row(children: [for (var a = 0; a < 3; a++) _axisChip(a, m.axis == a, () => _set(3, a.toDouble()))])),
          _labelled('結合', _switch(m.merge, (v) => _set(4, v ? 1 : 0), '触れ合う頂点をつなげる')),
        ],
      ]),
    );
  }

  Widget _labelled(String label, Widget child) => Padding(
        padding: const EdgeInsets.only(top: 6, right: 8),
        child: Row(children: [
          SizedBox(width: 42, child: Text(label, style: const TextStyle(fontSize: 12, color: _muted))),
          Expanded(child: child),
        ]),
      );

  Widget _axisChip(int a, bool on, VoidCallback onTap) => Expanded(
        child: Padding(
          padding: const EdgeInsets.symmetric(horizontal: 3),
          child: Semantics(
            button: true,
            selected: on,
            label: '${_axisNames[a]}軸',
            child: Material(
              color: on ? _axisColors[a].withValues(alpha: 0.24) : _chip,
              borderRadius: BorderRadius.circular(20),
              child: InkWell(
                borderRadius: BorderRadius.circular(20),
                onTap: onTap,
                child: SizedBox(
                  height: 44,
                  child: Center(
                    child: Text(_axisNames[a],
                        style: TextStyle(fontFamily: 'monospace', color: on ? _axisColors[a] : _muted, fontWeight: FontWeight.w600)),
                  ),
                ),
              ),
            ),
          ),
        ),
      );

  Widget _round(IconData icon, String tip, VoidCallback? onTap) => IconButton(
        tooltip: tip,
        onPressed: onTap,
        style: IconButton.styleFrom(backgroundColor: _chip, fixedSize: const Size(44, 44)),
        icon: Icon(icon, size: 16),
      );

  Widget _switch(bool value, ValueChanged<bool> onChanged, String hint) => Row(children: [
        Expanded(child: Text(hint, style: const TextStyle(fontSize: 12, color: _muted))),
        Switch(value: value, onChanged: onChanged),
      ]);
}
