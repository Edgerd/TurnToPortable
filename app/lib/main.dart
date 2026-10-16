// PortableXE — Flutter Material 3 桌面前端。
// 通过子进程调用 pxe-cli（build 子命令），解析其结构化进度行：
//   [pxe|progress|0.15|正在解压…]
// 其余输出按普通日志展示。

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';

void main() => runApp(const PortableXEApp());

const _scheme = ColorScheme.dark(
  primary: Color(0xFF4DA3FF),
  onPrimary: Color(0xFF002F63),
  secondary: Color(0xFFB9C7FF),
  onSecondary: Color(0xFF243782),
  surface: Color(0xFF111318),
  onSurface: Color(0xFFE2E2E9),
  surfaceContainerHighest: Color(0xFF1D2024),
  onSurfaceVariant: Color(0xFFC4C6CF),
  outline: Color(0xFF3A3D45),
  error: Color(0xFFFFB4AB),
);

class PortableXEApp extends StatelessWidget {
  const PortableXEApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'PortableXE',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorScheme: _scheme,
        useMaterial3: true,
        fontFamilyFallback: const ['Microsoft YaHei UI', 'Microsoft YaHei', 'SimHei'],
      ),
      home: const HomePage(),
    );
  }
}

enum PackKind { dir, zip, sevenZip }

extension PackKindLabel on PackKind {
  String get label => switch (this) { PackKind.dir => '文件夹', PackKind.zip => 'ZIP', PackKind.sevenZip => '7z' };
  String get flag => switch (this) { PackKind.dir => 'dir', PackKind.zip => 'zip', PackKind.sevenZip => '7z' };
}

class _LogLine {
  final String text;
  final bool error;
  _LogLine(this.text, {this.error = false});
}

class HomePage extends StatefulWidget {
  const HomePage({super.key});

  @override
  State<HomePage> createState() => _HomePageState();
}

class _HomePageState extends State<HomePage> {
  final _inputCtrl = TextEditingController();
  final _nameCtrl = TextEditingController();
  final _outCtrl = TextEditingController();
  final _logCtrl = ScrollController();

  PackKind _pack = PackKind.dir;
  double _progress = 0;
  String _status = '准备就绪';
  final List<_LogLine> _log = [];
  bool _busy = false;
  Process? _proc;

  @override
  void initState() {
    super.initState();
    _outCtrl.text = Platform.environment['USERPROFILE'] != null
        ? '${Platform.environment['USERPROFILE']}\\Desktop'
        : Directory.current.path;
  }

  @override
  void dispose() {
    _proc?.kill();
    _inputCtrl.dispose();
    _nameCtrl.dispose();
    _outCtrl.dispose();
    _logCtrl.dispose();
    super.dispose();
  }

  Future<void> _pickFile() async {
    const types = XTypeGroup(
      label: '安装包',
      extensions: ['exe', 'msi', 'msix', 'appx', '7z', 'zip', 'rar'],
    );
    final f = await openFile(acceptedTypeGroups: [types]);
    if (f == null) return;
    _inputCtrl.text = f.path;
    if (_nameCtrl.text.trim().isEmpty) {
      _nameCtrl.text = _deriveName(f.name);
    }
    setState(() {});
  }

  Future<void> _pickDir() async {
    final d = await getDirectoryPath();
    if (d == null) return;
    _outCtrl.text = d;
    setState(() {});
  }

  String _deriveName(String fileName) {
    var stem = fileName.contains('.') ? fileName.substring(0, fileName.lastIndexOf('.')) : fileName;
    for (final t in ['setup', 'install', 'installer']) {
      stem = stem.toLowerCase().replaceAll(t, '');
    }
    stem = stem.replaceAll(RegExp(r'^[-_\s]+|[-_\s]+$'), '');
    if (stem.isEmpty) return 'App';
    return stem[0].toUpperCase() + stem.substring(1);
  }

  String? _cliPath() {
    // 与 pxe.exe 同目录分发；开发时可放 PATH
    final self = Platform.resolvedExecutable;
    final dir = File(self).parent.path;
    for (final c in ['$dir/pxe.exe', '$dir/pxe']) {
      if (File(c).existsSync()) return c;
    }
    return null;
  }

  Future<void> _start() async {
    if (_inputCtrl.text.trim().isEmpty || _nameCtrl.text.trim().isEmpty || _outCtrl.text.trim().isEmpty) {
      _addLog('请先填写安装包文件、应用名称和输出目录', error: true);
      return;
    }
    setState(() {
      _busy = true;
      _progress = 0;
      _status = '启动中…';
      _log.clear();
    });
    _scrollToBottom();

    final args = [
      'build',
      _inputCtrl.text.trim(),
      '-n', _nameCtrl.text.trim(),
      '-o', _outCtrl.text.trim(),
      '--pack', _pack.flag,
    ];
    final cli = _cliPath();
    if (cli == null) {
      setState(() => _busy = false);
      _addLog('✖ 找不到 pxe.exe，请将其放在本程序同目录下', error: true);
      return;
    }
    try {
      final proc = await Process.start(cli, args, runInShell: false);
      _proc = proc;
      proc.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen(_onLine);
      proc.stderr.transform(utf8.decoder).transform(const LineSplitter()).listen((l) => _onLine(l, stderr: true));
      final code = await proc.exitCode;
      setState(() {
        _busy = false;
        _status = code == 0 ? '完成' : '失败（退出码 $code）';
      });
      _addLog(code == 0 ? '✔ 全部完成' : '✖ 进程退出码 $code', error: code != 0);
      _scrollToBottom();
    } catch (e) {
      setState(() => _busy = false);
      _addLog('✖ 无法启动 pxe-cli：$e', error: true);
    }
  }

  void _onLine(String line, {bool stderr = false}) {
    // 结构化进度行：[pxe|progress|0.55|消息]
    final m = RegExp(r'^\[pxe\|progress\|([\d.]+)\|(.*)\]$').firstMatch(line.trim());
    if (m != null) {
      setState(() {
        _progress = double.tryParse(m.group(1)!) ?? _progress;
        _status = m.group(2)!;
      });
      return;
    }
    final t = line.trim();
    if (t.isEmpty) return;
    final error = stderr || t.startsWith('✖') || t.toLowerCase().contains('error') || t.toLowerCase().contains('failed');
    _addLog(t, error: error);
    _scrollToBottom();
  }

  void _addLog(String text, {bool error = false}) {
    setState(() => _log.add(_LogLine(text, error: error)));
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_logCtrl.hasClients) {
        _logCtrl.animateTo(
          _logCtrl.position.maxScrollExtent,
          duration: const Duration(milliseconds: 150),
          curve: Curves.easeOut,
        );
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 640),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 32, vertical: 28),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Text('PortableXE', style: theme.textTheme.headlineMedium?.copyWith(fontWeight: FontWeight.w600)),
                Text('把 Windows 安装包变成便携应用', style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
                const SizedBox(height: 28),
                _Field(
                  label: '安装包文件',
                  controller: _inputCtrl,
                  hint: '选择 .exe / .msi / 压缩包…',
                  action: FilledButton.tonalIcon(
                    onPressed: _busy ? null : _pickFile,
                    icon: const Icon(Icons.folder_open, size: 18),
                    label: const Text('浏览'),
                  ),
                ),
                const SizedBox(height: 16),
                _Field(
                  label: '应用名称',
                  controller: _nameCtrl,
                  hint: '便携应用的文件夹名',
                ),
                const SizedBox(height: 16),
                _Field(
                  label: '输出目录',
                  controller: _outCtrl,
                  action: FilledButton.tonalIcon(
                    onPressed: _busy ? null : _pickDir,
                    icon: const Icon(Icons.create_new_folder_outlined, size: 18),
                    label: const Text('浏览'),
                  ),
                ),
                const SizedBox(height: 16),
                Row(
                  children: [
                    Text('打包为', style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
                    const SizedBox(width: 12),
                    SegmentedButton<PackKind>(
                      segments: [
                        ButtonSegment(value: PackKind.dir, label: const Text('文件夹'), icon: const Icon(Icons.folder, size: 18)),
                        ButtonSegment(value: PackKind.zip, label: const Text('ZIP'), icon: const Icon(Icons.archive_outlined, size: 18)),
                        ButtonSegment(value: PackKind.sevenZip, label: const Text('7z'), icon: const Icon(Icons.inventory_2_outlined, size: 18)),
                      ],
                      selected: {_pack},
                      onSelectionChanged: _busy ? null : (s) => setState(() => _pack = s.first),
                      showSelectedIcon: false,
                    ),
                  ],
                ),
                const SizedBox(height: 28),
                Row(
                  children: [
                    FilledButton.icon(
                      onPressed: _busy ? null : _start,
                      icon: _busy
                          ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                          : const Icon(Icons.bolt, size: 20),
                      label: Text(_busy ? '正在处理…' : '创建便携应用'),
                      style: FilledButton.styleFrom(
                        minimumSize: const Size(180, 44),
                        textStyle: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
                      ),
                    ),
                    const SizedBox(width: 20),
                    Expanded(
                      child: Column(
                        crossAxisAlignment: CrossAxisAlignment.start,
                        children: [
                          ClipRRect(
                            borderRadius: BorderRadius.circular(4),
                            child: LinearProgressIndicator(
                              value: _progress > 0 ? _progress : null,
                              minHeight: 8,
                              backgroundColor: theme.colorScheme.surfaceContainerHighest,
                            ),
                          ),
                          const SizedBox(height: 6),
                          Text(
                            _progress > 0 ? '${(_progress * 100).round()}% · $_status' : _status,
                            style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                          ),
                        ],
                      ),
                    ),
                  ],
                ),
                if (_log.isNotEmpty) ...[
                  const SizedBox(height: 24),
                  Expanded(
                    child: Container(
                      width: double.infinity,
                      decoration: BoxDecoration(
                        color: const Color(0xFF0C0E12),
                        borderRadius: BorderRadius.circular(12),
                        border: Border.all(color: theme.colorScheme.outline.withValues(alpha: 0.4)),
                      ),
                      child: Scrollbar(
                        controller: _logCtrl,
                        child: SingleChildScrollView(
                          controller: _logCtrl,
                          padding: const EdgeInsets.all(14),
                          child: Column(
                            crossAxisAlignment: CrossAxisAlignment.start,
                            children: [
                              for (final l in _log)
                                Padding(
                                  padding: const EdgeInsets.only(bottom: 3),
                                  child: Text(
                                    l.text,
                                    style: TextStyle(
                                      fontFamily: 'Consolas',
                                      fontSize: 12,
                                      color: l.error ? theme.colorScheme.error : const Color(0xFF9FE29F),
                                    ),
                                  ),
                                ),
                            ],
                          ),
                        ),
                      ),
                    ),
                  ),
                ],
              ],
            ),
          ),
        ),
      ),
    );
  }
}

class _Field extends StatelessWidget {
  final String label;
  final TextEditingController controller;
  final String? hint;
  final Widget? action;

  const _Field({required this.label, required this.controller, this.hint, this.action});

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        SizedBox(width: 88, child: Text(label, style: Theme.of(context).textTheme.bodyMedium)),
        const SizedBox(width: 12),
        Expanded(
          child: TextField(
            controller: controller,
            style: const TextStyle(fontSize: 14),
            decoration: InputDecoration(
              hintText: hint,
              isDense: true,
              filled: true,
              fillColor: Theme.of(context).colorScheme.surfaceContainerHighest.withValues(alpha: 0.5),
              contentPadding: const EdgeInsets.symmetric(horizontal: 14, vertical: 12),
              border: OutlineInputBorder(borderRadius: BorderRadius.circular(10), borderSide: BorderSide.none),
            ),
          ),
        ),
        if (action != null) ...[const SizedBox(width: 10), action!],
      ],
    );
  }
}
