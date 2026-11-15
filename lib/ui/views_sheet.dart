import 'package:flutter/material.dart';

import 'presets.dart';
import 'viewport.dart';

const _muted = Color(0xFF8E8E95);

/// Name of the view kept automatically for every saved capture.
const lastShotName = '前回の撮影';

/// Saved camera positions of the work: save the current one, jump to one, rename, delete.
class ViewsSheet extends StatefulWidget {
  const ViewsSheet({super.key, required this.viewport});
  final ModelViewportController viewport;

  @override
  State<ViewsSheet> createState() => _ViewsSheetState();
}

class _ViewsSheetState extends State<ViewsSheet> {
  ModelViewportController get _vp => widget.viewport;

  Future<void> _save() async {
    final name = await askName(context, title: '今の視点を保存', initial: '視点 ${_vp.views().length + 1}');
    if (name == null) return;
    _vp.viewSave(name);
    setState(() {});
  }

  Future<void> _menu(int i, String name) async {
    final action = await showModalBottomSheet<String>(
      context: context,
      backgroundColor: const Color(0xFF252528),
      builder: (ctx) => SafeArea(
        child: Column(mainAxisSize: MainAxisSize.min, children: [
          ListTile(title: Text(name, style: const TextStyle(fontWeight: FontWeight.w600))),
          ListTile(leading: const Icon(Icons.edit_outlined), title: const Text('名前を変更'), onTap: () => Navigator.pop(ctx, 'rename')),
          ListTile(leading: const Icon(Icons.update), title: const Text('今の視点で上書き'), onTap: () => Navigator.pop(ctx, 'overwrite')),
          ListTile(leading: const Icon(Icons.delete_outline), title: const Text('削除'), onTap: () => Navigator.pop(ctx, 'delete')),
        ]),
      ),
    );
    if (!mounted || action == null) return;
    switch (action) {
      case 'rename':
        final n = await askName(context, title: '名前を変更', initial: name, action: '変更');
        if (n != null) _vp.viewRename(i, n);
      case 'overwrite':
        _vp.viewSave(name, replace: true);
      case 'delete':
        _vp.viewDelete(i);
    }
    setState(() {});
  }

  @override
  Widget build(BuildContext context) {
    final views = _vp.views();
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(8, 12, 8, 16),
        child: Column(mainAxisSize: MainAxisSize.min, crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(children: [
            const SizedBox(width: 12),
            const Expanded(child: Text('視点', style: TextStyle(fontSize: 16, fontWeight: FontWeight.w600))),
            TextButton.icon(onPressed: _save, icon: const Icon(Icons.bookmark_add_outlined, size: 18), label: const Text('今の視点を保存')),
          ]),
          if (views.isEmpty)
            const Padding(
              padding: EdgeInsets.fromLTRB(12, 8, 12, 12),
              child: Text('カメラの位置と向きを名前を付けて保存し、あとでワンタップで戻れます。撮影で保存した位置は「前回の撮影」に残ります。',
                  style: TextStyle(fontSize: 12, color: _muted)),
            )
          else
            Flexible(
              child: ListView.builder(
                shrinkWrap: true,
                itemCount: views.length,
                itemBuilder: (_, i) {
                  final v = views[i];
                  return ListTile(
                    leading: Icon(v.name == lastShotName ? Icons.photo_camera_outlined : Icons.bookmark_border),
                    title: Text(v.name),
                    subtitle: Text(v.ortho ? '平行投影' : '透視投影', style: const TextStyle(fontSize: 11, color: _muted)),
                    onTap: () {
                      _vp.viewApply(i);
                      Navigator.pop(context);
                    },
                    trailing: IconButton(tooltip: 'メニュー', icon: const Icon(Icons.more_horiz), onPressed: () => _menu(i, v.name)),
                  );
                },
              ),
            ),
        ]),
      ),
    );
  }
}
