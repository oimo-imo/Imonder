import 'package:flutter/material.dart';

import '../storage/work_store.dart';

/// A horseshoe magnet (Material Icons has none).
class MagnetIcon extends StatelessWidget {
  const MagnetIcon({super.key, required this.color, this.size = 21});
  final Color color;
  final double size;

  @override
  Widget build(BuildContext context) => CustomPaint(size: Size.square(size), painter: _MagnetPainter(color));
}

class _MagnetPainter extends CustomPainter {
  _MagnetPainter(this.color);
  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final s = size.width;
    final stroke = Paint()
      ..color = color
      ..style = PaintingStyle.stroke
      ..strokeWidth = s * 0.17
      ..strokeCap = StrokeCap.butt;
    // U opening upwards: two legs joined by a half circle at the bottom.
    final r = s * 0.27, cx = s / 2, top = s * 0.12, bend = s * 0.55;
    final path = Path()
      ..moveTo(cx - r, top)
      ..lineTo(cx - r, bend)
      ..arcToPoint(Offset(cx + r, bend), radius: Radius.circular(r), clockwise: false)
      ..lineTo(cx + r, top);
    canvas.drawPath(path, stroke);
    // Pole tips.
    final tip = Paint()..color = color;
    canvas.drawRect(Rect.fromLTWH(cx - r - s * 0.085, top - s * 0.02, s * 0.17, s * 0.14), tip);
    canvas.drawRect(Rect.fromLTWH(cx + r - s * 0.085, top - s * 0.02, s * 0.17, s * 0.14), tip);
  }

  @override
  bool shouldRepaint(_MagnetPainter old) => old.color != color;
}

/// Horizontal list of presets with thumbnails. Long-press for rename / delete.
class PresetStrip extends StatefulWidget {
  const PresetStrip({super.key, required this.store, required this.onPick});
  final WorkStore store;
  final ValueChanged<WorkInfo> onPick;

  @override
  State<PresetStrip> createState() => _PresetStripState();
}

class _PresetStripState extends State<PresetStrip> {
  List<WorkInfo>? _items;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    final list = await widget.store.list();
    if (mounted) setState(() => _items = list);
  }

  Future<void> _menu(WorkInfo w) async {
    final action = await showModalBottomSheet<String>(
      context: context,
      backgroundColor: const Color(0xFF252528),
      builder: (ctx) => SafeArea(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          ListTile(title: Text(w.name, style: const TextStyle(fontWeight: FontWeight.w600))),
          ListTile(leading: const Icon(Icons.edit_outlined), title: const Text('名前を変更'), onTap: () => Navigator.pop(ctx, 'rename')),
          ListTile(leading: const Icon(Icons.delete_outline), title: const Text('削除'), onTap: () => Navigator.pop(ctx, 'delete')),
        ]),
      ),
    );
    if (!mounted || action == null) return;
    if (action == 'rename') {
      final name = await askName(context, title: '名前を変更', initial: w.name, action: '変更');
      if (name != null) await widget.store.rename(w.id, name);
    } else {
      await widget.store.delete(w.id);
    }
    await _reload();
  }

  @override
  Widget build(BuildContext context) {
    final items = _items;
    if (items == null) return const SizedBox(height: 124); // reading the folder takes a moment
    if (items.isEmpty) {
      return const Padding(
        padding: EdgeInsets.symmetric(vertical: 12),
        child: Text('シーン一覧の「プリセットに登録」で、表示中のオブジェクトを登録できます',
            style: TextStyle(fontSize: 12, color: Color(0xFF8E8E95))),
      );
    }
    return SizedBox(
      height: 124,
      child: ListView.separated(
        scrollDirection: Axis.horizontal,
        itemCount: items.length,
        separatorBuilder: (_, _) => const SizedBox(width: 10),
        itemBuilder: (_, i) {
          final w = items[i];
          return Semantics(
            button: true,
            label: 'プリセット ${w.name}',
            child: InkWell(
              borderRadius: BorderRadius.circular(14),
              onTap: () => widget.onPick(w),
              onLongPress: () => _menu(w),
              child: SizedBox(
                width: 96,
                child: Column(children: [
                  Container(
                    width: 96,
                    height: 96,
                    clipBehavior: Clip.antiAlias,
                    decoration: BoxDecoration(color: const Color(0xFF303034), borderRadius: BorderRadius.circular(14)),
                    child: w.thumbnail == null
                        ? const Icon(Icons.bookmarks_outlined, color: Color(0xFF5E5E65))
                        : Image.memory(w.thumbnail!.readAsBytesSync(), fit: BoxFit.cover, gaplessPlayback: true),
                  ),
                  const SizedBox(height: 4),
                  Text(w.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 12)),
                ]),
              ),
            ),
          );
        },
      ),
    );
  }
}

/// Asks for a name; null when cancelled or blank.
Future<String?> askName(BuildContext context, {required String title, String initial = '', String action = '登録'}) async {
  final controller = TextEditingController(text: initial);
  final name = await showDialog<String>(
    context: context,
    builder: (ctx) => AlertDialog(
      title: Text(title),
      content: TextField(controller: controller, autofocus: true, maxLength: 40),
      actions: [
        TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('キャンセル')),
        FilledButton(onPressed: () => Navigator.pop(ctx, controller.text), child: Text(action)),
      ],
    ),
  );
  final trimmed = name?.trim();
  return trimmed == null || trimmed.isEmpty ? null : trimmed;
}

/// "New work" picker: a blank work or one of the presets. Returns null when cancelled,
/// '' for blank, otherwise the preset id.
Future<String?> pickNewWork(BuildContext context, WorkStore presets) {
  return showModalBottomSheet<String>(
    context: context,
    isScrollControlled: true,
    backgroundColor: const Color(0xFF252528),
    shape: const RoundedRectangleBorder(borderRadius: BorderRadius.vertical(top: Radius.circular(26))),
    builder: (ctx) => SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(16, 16, 16, 20),
        child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          const Text('新しい作品', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
          const SizedBox(height: 12),
          SizedBox(
            width: double.infinity,
            height: 48,
            child: OutlinedButton.icon(
              onPressed: () => Navigator.pop(ctx, ''),
              icon: const Icon(Icons.add),
              label: const Text('空の作品（立方体ひとつ）'),
            ),
          ),
          const SizedBox(height: 16),
          const Text('プリセットから始める', style: TextStyle(fontSize: 13, color: Color(0xFF8E8E95))),
          const SizedBox(height: 8),
          PresetStrip(store: presets, onPick: (w) => Navigator.pop(ctx, w.id)),
        ]),
      ),
    ),
  );
}
