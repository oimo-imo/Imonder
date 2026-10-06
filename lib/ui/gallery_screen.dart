import 'package:flutter/material.dart';

import '../storage/work_store.dart';

/// What the gallery asks the editor to do when it closes.
sealed class GalleryChoice {
  const GalleryChoice();
}

class OpenWork extends GalleryChoice {
  const OpenWork(this.id);
  final String id;
}

class NewWork extends GalleryChoice {
  const NewWork();
}

/// The open work was deleted; the editor should start a fresh one.
class CurrentDeleted extends GalleryChoice {
  const CurrentDeleted();
}

String formatDate(DateTime d) {
  String two(int n) => n.toString().padLeft(2, '0');
  return '${d.year}/${two(d.month)}/${two(d.day)} ${two(d.hour)}:${two(d.minute)}';
}

class GalleryScreen extends StatefulWidget {
  const GalleryScreen({super.key, required this.store, required this.currentId});
  final WorkStore store;
  final String? currentId;

  @override
  State<GalleryScreen> createState() => _GalleryScreenState();
}

class _GalleryScreenState extends State<GalleryScreen> {
  List<WorkInfo>? _works;

  @override
  void initState() {
    super.initState();
    _reload();
  }

  Future<void> _reload() async {
    final list = await widget.store.list();
    if (mounted) setState(() => _works = list);
  }

  Future<void> _rename(WorkInfo w) async {
    final controller = TextEditingController(text: w.name);
    final name = await showDialog<String>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('名前を変更'),
        content: TextField(controller: controller, autofocus: true, maxLength: 60),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx), child: const Text('キャンセル')),
          FilledButton(onPressed: () => Navigator.pop(ctx, controller.text), child: const Text('変更')),
        ],
      ),
    );
    if (name != null) {
      await widget.store.rename(w.id, name);
      await _reload();
    }
  }

  Future<void> _delete(WorkInfo w) async {
    final ok = await showDialog<bool>(
      context: context,
      builder: (ctx) => AlertDialog(
        title: const Text('削除しますか？'),
        content: Text('「${w.name}」を削除します。元に戻せません。'),
        actions: [
          TextButton(onPressed: () => Navigator.pop(ctx, false), child: const Text('キャンセル')),
          FilledButton(onPressed: () => Navigator.pop(ctx, true), child: const Text('削除')),
        ],
      ),
    );
    if (ok != true) return;
    await widget.store.delete(w.id);
    if (!mounted) return;
    if (w.id == widget.currentId) {
      Navigator.pop(context, const CurrentDeleted());
    } else {
      await _reload();
    }
  }

  @override
  Widget build(BuildContext context) {
    final accent = Theme.of(context).colorScheme.primary;
    final works = _works;
    return Scaffold(
      backgroundColor: const Color(0xFF1E1E21),
      appBar: AppBar(
        backgroundColor: const Color(0xFF161618),
        title: const Text('ギャラリー'),
        actions: [
          Padding(
            padding: const EdgeInsets.only(right: 8),
            child: FilledButton.icon(
              onPressed: () => Navigator.pop(context, const NewWork()),
              icon: const Icon(Icons.add, size: 20),
              label: const Text('新規'),
            ),
          ),
        ],
      ),
      body: works == null
          ? const Center(child: CircularProgressIndicator())
          : LayoutBuilder(builder: (context, c) {
              final columns = c.maxWidth >= 900 ? 4 : (c.maxWidth >= 600 ? 3 : 2);
              return GridView.builder(
                padding: const EdgeInsets.all(16),
                gridDelegate: SliverGridDelegateWithFixedCrossAxisCount(
                  crossAxisCount: columns,
                  mainAxisSpacing: 16,
                  crossAxisSpacing: 16,
                  childAspectRatio: 0.82,
                ),
                itemCount: works.length,
                itemBuilder: (context, i) => _card(works[i], accent),
              );
            }),
    );
  }

  Widget _card(WorkInfo w, Color accent) {
    final current = w.id == widget.currentId;
    return Semantics(
      label: w.name,
      button: true,
      child: InkWell(
        borderRadius: BorderRadius.circular(18),
        onTap: () => Navigator.pop(context, OpenWork(w.id)),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Expanded(
            child: Container(
              decoration: BoxDecoration(
                color: const Color(0xFF2A2A2E),
                borderRadius: BorderRadius.circular(18),
                border: current ? Border.all(color: accent, width: 2) : null,
              ),
              clipBehavior: Clip.antiAlias,
              child: w.thumbnail == null
                  ? const Center(child: Icon(Icons.view_in_ar, color: Color(0xFF5E5E65), size: 36))
                  : Image.memory(w.thumbnail!.readAsBytesSync(), fit: BoxFit.cover, gaplessPlayback: true),
            ),
          ),
          const SizedBox(height: 8),
          Row(children: [
            Expanded(child: Text(w.name, maxLines: 1, overflow: TextOverflow.ellipsis, style: const TextStyle(fontSize: 14))),
            SizedBox(
              width: 32,
              height: 32,
              child: PopupMenuButton<String>(
                padding: EdgeInsets.zero,
                tooltip: 'メニュー',
                icon: const Icon(Icons.more_horiz, size: 20, color: Color(0xFF8E8E95)),
                onSelected: (v) => v == 'rename' ? _rename(w) : _delete(w),
                itemBuilder: (_) => const [
                  PopupMenuItem(value: 'rename', child: Text('名前を変更')),
                  PopupMenuItem(value: 'delete', child: Text('削除')),
                ],
              ),
            ),
          ]),
          Text(formatDate(w.updated), style: const TextStyle(fontSize: 11, color: Color(0xFF8E8E95), fontFamily: 'monospace')),
        ]),
      ),
    );
  }
}
