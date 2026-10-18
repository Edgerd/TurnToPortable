// PortableXE — Flutter Material 3 桌面前端。
// 通过子进程调用 pxe-cli（build 子命令），解析其结构化进度行：
//   [pxe|progress|0.15|正在解压…]
// 其余输出按普通日志展示，错误行做通俗化翻译。

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:file_selector/file_selector.dart';
import 'package:flutter/material.dart';
import 'package:path_provider/path_provider.dart';

void main() async {
  WidgetsFlutterBinding.ensureInitialized();
  await Settings.load();
  runApp(const PortableXEApp());
}

// ---------------- 设置（真实生效，落盘持久化） ----------------

class Settings {
  /// 1-9，影响 zip/7z 打包体积与速度（写入临时 toml 传给 pxe-cli）
  static int compression = 6;

  /// 外部解压工具超时（分钟），默认 5（写入 toml 的 timeout_seconds）
  static int timeoutMin = 5;

  /// 优先用 pxe-stub.exe 原生启动器（无黑窗）；关掉则生成 bat 启动器（可改参数）
  static bool nativeStub = true;

  /// 在 Redist/ 放一份 vc_redist 提示说明
  static bool redistNote = true;

  /// 构建成功后自动打开输出文件夹
  static bool openWhenDone = false;

  /// 构建成功后自动启动生成的便携应用
  static bool runWhenDone = false;

  /// 输出目录已存在同名应用时直接覆盖（关闭则报错提示改名）
  static bool overwrite = true;

  /// 设置文件所在目录（按平台取应用支持目录，跨平台正确）
  static Directory? _dir;

  static Future<void> _ensureDir() async {
    _dir ??= await getApplicationSupportDirectory();
  }

  static File get _file {
    final base = _dir?.path ?? '.';
    return File('$base/PortableXE/settings.json');
  }

  static Future<void> load() async {
    await _ensureDir();
    try {
      final m = jsonDecode(await _file.readAsString()) as Map<String, dynamic>;
      compression = ((m['compression'] as num?) ?? 6).clamp(1, 9).toInt();
      timeoutMin = ((m['timeoutMin'] as num?) ?? 5).clamp(1, 120).toInt();
      nativeStub = m['nativeStub'] ?? true;
      redistNote = m['redistNote'] ?? true;
      openWhenDone = m['openWhenDone'] ?? false;
      runWhenDone = m['runWhenDone'] ?? false;
      overwrite = m['overwrite'] ?? true;
    } on FileSystemException catch (e) {
      debugPrint('settings load failed (using defaults): $e');
    } on FormatException catch (e) {
      debugPrint('settings file corrupted (using defaults): $e');
    }
  }

  static Future<void> save() async {
    await _ensureDir();
    try {
      await _file.writeAsString(jsonEncode({
        'compression': compression,
        'timeoutMin': timeoutMin,
        'nativeStub': nativeStub,
        'redistNote': redistNote,
        'openWhenDone': openWhenDone,
        'runWhenDone': runWhenDone,
        'overwrite': overwrite,
      }));
    } on FileSystemException catch (e) {
      debugPrint('settings save failed: $e');
    }
  }

  /// 生成 pxe-cli 的 toml 配置 —— 设置项的实际消费者
  static String toToml(String outDir) {
    final esc = outDir.replaceAll('\\', '\\\\');
    return '''
output_dir = "$esc"
compression_level = $compression
timeout_seconds = ${timeoutMin * 60}
bundle_redist_note = $redistNote
prefer_native_stub = $nativeStub
''';
  }
}

// ---------------- 主题 ----------------

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
        cardTheme: const CardThemeData(
          elevation: 0,
          color: Color(0xFF191C22),
          shape: RoundedRectangleBorder(
            borderRadius: BorderRadius.all(Radius.circular(14)),
            side: BorderSide(color: Color(0xFF2A2E36)),
          ),
          margin: EdgeInsets.zero,
        ),
        sliderTheme: const SliderThemeData(showValueIndicator: ShowValueIndicator.onDrag),
        fontFamilyFallback: const ['Microsoft YaHei UI', 'Microsoft YaHei', 'SimHei'],
      ),
      home: const HomePage(),
    );
  }
}

// ---------------- 数据模型 ----------------

enum PackKind { dir, zip, sevenZip }

extension PackKindLabel on PackKind {
  String get label => switch (this) { PackKind.dir => '文件夹', PackKind.zip => 'ZIP', PackKind.sevenZip => '7z' };
  String get flag => switch (this) { PackKind.dir => 'dir', PackKind.zip => 'zip', PackKind.sevenZip => 'seven-zip' };
  String get hint => switch (this) {
        PackKind.dir => '生成标准便携目录，可直接放入 U 盘',
        PackKind.zip => '打成单个 .zip，便于网络分发',
        PackKind.sevenZip => '打成 .7z，体积最小（压缩级别见设置）',
      };
}

class _LogLine {
  final String text;
  final bool error;
  final bool success;
  _LogLine(this.text, {this.error = false, this.success = false});
}

// ---------------- 报错通俗化 ----------------

String friendly(String raw) {
  final t = raw.trim();
  final rules = [
    (RegExp(r'Unexpected setup data version|Stream error while parsing setup headers', caseSensitive: false),
        '这个安装包使用了较新版本的 Inno Setup（6.4+），当前内置的 innoextract 1.9 还不支持。可以等待工具更新，或用「独立应用目录」方式先手动安装再打包。'),
    (RegExp(r'7-Zip not found|needs 7z\.exe', caseSensitive: false),
        '这种安装包（NSIS）需要 7-Zip 组件解包。新版已随程序附带 7z.exe；如果你是自己单独下载的 pxe.exe，请把 7z.exe 和 7z.dll 放到程序同一目录（或安装 7-Zip 后重试）。'),
    (RegExp(r'解包失败（', caseSensitive: false),
        '解包失败了：这个安装包可能是加密的企业分发包、已损坏，或使用了暂不支持的打包技术。可以先用 7-Zip 手动打开确认里面是什么，再用「独立应用目录」方式打包。'),
    (RegExp(r'innoextract.*not found|innoextract not', caseSensitive: false),
        '这种 Inno Setup 安装包需要 innoextract 工具辅助解包。请从 constexpr.org/innoextract 下载，把 innoextract.exe 放到本程序同一目录后重试。'),
    (RegExp(r'unrar.*not found', caseSensitive: false),
        'RAR 压缩包需要 unrar 工具。请下载 unrar.exe 放到本程序同一目录后重试。'),
    (RegExp(r'msiexec', caseSensitive: false),
        'MSI 解包失败。可能原因：① 安装包损坏；② 需要管理员权限——试试右键本程序「以管理员身份运行」；③ 该 MSI 是加密的企业分发包，无法直接解包。'),
    (RegExp(r'timed out after', caseSensitive: false),
        '解包超时了。文件越大需要越久，可以在「设置 → 解包超时」里调大后重试。'),
    (RegExp(r'output failed validation', caseSensitive: false),
        '解包出来的内容不完整（没找到可执行文件）。这个包可能是加密安装器或不支持的类型。'),
    (RegExp(r'exited with', caseSensitive: false),
        '外部解包工具执行失败。如果反复出现，请把这个安装包用 7-Zip 手动解压后，把解出的文件夹作为「独立应用目录」重新打包。'),
    (RegExp(r'access is denied|拒绝访问|os error 5', caseSensitive: false),
        '没有访问权限，通常是文件被占用或被杀毒软件拦截。请关闭占用程序、把本目录加入杀毒白名单后重试。'),
    (RegExp(r'no usable .exe found', caseSensitive: false),
        '解包内容里没有找到任何 .exe 程序。如果是绿色软件，请直接选中它的主程序文件夹来打包。'),
    (RegExp(r'not found in PATH', caseSensitive: false),
        '缺少必要的解包工具。请按日志上方的提示安装对应工具。'),
    (RegExp(r'broken ZIP|invalid', caseSensitive: false),
        '文件已损坏或不是完整的压缩包，请重新下载。'),
    (RegExp(r'os error 112', caseSensitive: false),
        '磁盘空间不足，请清理输出目录所在磁盘后重试。'),
  ];
  for (final (re, msg) in rules) {
    if (re.hasMatch(t)) return msg;
  }
  return t;
}

// ---------------- 主界面 ----------------

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
  bool _logOpen = false;
  bool _outTouched = false; // 用户是否手动改过输出目录；没改过就跟随安装包所在目录
  Process? _proc;

  String? _builtDir;   // [pxe] portable app: ...
  String? _builtExe;   // [pxe] native stub launcher: ...

  @override
  void initState() {
    super.initState();
    _outCtrl.text = Platform.environment['USERPROFILE'] != null
        ? '${Platform.environment['USERPROFILE']}\\PortableApps'
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
      label: '支持的文件',
      extensions: ['exe', 'msi', 'msix', 'appx', '7z', 'zip', 'rar', 'cab', 'tar', 'gz', 'tgz'],
    );
    final f = await openFile(acceptedTypeGroups: [types]);
    if (f == null) return;
    final p = File(f.path);
    if (!p.existsSync()) {
      _addLog('选中的文件不存在，可能已被移动或删除', error: true);
      return;
    }
    _inputCtrl.text = f.path;
    if (_nameCtrl.text.trim().isEmpty) _nameCtrl.text = _deriveName(f.name);
    if (!_outTouched) {
      final parent = File(f.path).parent.path;
      if (parent.isNotEmpty) _outCtrl.text = parent;
    }
    setState(() {});
    _precheck();
  }

  Future<void> _pickDir() async {
    final d = await getDirectoryPath();
    if (d == null) return;
    _outCtrl.text = d;
    _outTouched = true;
    setState(() {});
  }

  String _deriveName(String fileName) {
    var stem = fileName.contains('.') ? fileName.substring(0, fileName.lastIndexOf('.')) : fileName;
    // 大小写不敏感移除后缀词，但保留原始大小写（CrystalDiskInfo 不能变成 crystaldiskinfo）
    for (final t in ['installer', 'install', 'setup', '-x64', '-x86', '-win64', '-win32', '_x64']) {
      final idx = stem.toLowerCase().indexOf(t);
      if (idx >= 0) stem = stem.substring(0, idx) + stem.substring(idx + t.length);
    }
    stem = stem.replaceAll(RegExp(r'^[-_.\s]+|[-_.\s]+$'), '').replaceAll(RegExp(r'[-_\s]+'), ' ');
    if (stem.isEmpty) return 'App';
    final capped = stem[0].toUpperCase() + stem.substring(1);
    return capped.split(' ').map((w) => w.isEmpty ? w : w[0].toUpperCase() + w.substring(1)).join(' ');
  }

  /// 选择的文件能否被当前环境处理 —— 提前给出行话提示
  void _precheck() {
    final path = _inputCtrl.text.toLowerCase();
    final name = Platform.environment['PATH'] ?? '';
    if (path.endsWith('.rar') && !name.contains('unrar')) {
      _addLog('提示：RAR 包需要 unrar.exe（放本程序目录）。没有的话可先用 7-Zip 解压再打包目录。');
    }
    if (path.endsWith('.msi') && !Platform.isWindows) {
      _addLog('提示：MSI 解包只能在 Windows 上进行。', error: true);
    }
  }

  String? _cliPath() {
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
    if (!File(_inputCtrl.text.trim()).existsSync()) {
      _addLog('安装包文件不存在：${_inputCtrl.text.trim()}\n文件可能被移动、删除或路径写错了', error: true);
      return;
    }
    // 覆盖检查
    final target = Directory('${_outCtrl.text.trim().replaceAll(RegExp(r'\\+$'), '')}\\${_nameCtrl.text.trim()}_Portable');
    if (!Settings.overwrite && target.existsSync()) {
      _addLog('输出目录里已经有「${_nameCtrl.text.trim()}_Portable」。\n可以在设置里开启「覆盖已有应用」，或换个应用名称。', error: true);
      return;
    }

    // 设置 → 临时 toml（设置项的真实消费者是 pxe-cli）
    final cfgFile = File('${Directory.systemTemp.path}/portablexe-gui.toml');
    await cfgFile.writeAsString(Settings.toToml(_outCtrl.text.trim()));

    setState(() {
      _busy = true;
      _progress = 0;
      _status = '启动中…';
      _log.clear();
      _logOpen = true;
      _builtDir = null;
      _builtExe = null;
    });
    _scrollToBottom();

    final args = [
      'build',
      _inputCtrl.text.trim(),
      '-n', _nameCtrl.text.trim(),
      '--config', cfgFile.path,
      '--pack', _pack.flag,
    ];
    final cli = _cliPath();
    if (cli == null) {
      setState(() => _busy = false);
      _addLog('找不到 pxe.exe —— 它必须和 portablexe.exe 放在同一个文件夹里。\n如果是从压缩包解出来的，请确认三个 exe 都在。', error: true);
      return;
    }
    try {
      final proc = await Process.start(cli, args, runInShell: false);
      _proc = proc;
      proc.stdout.transform(utf8.decoder).transform(const LineSplitter()).listen(_onLine);
      proc.stderr.transform(utf8.decoder).transform(const LineSplitter()).listen((l) => _onLine(l, stderr: true));
      final code = await proc.exitCode;
      final ok = code == 0;
      setState(() {
        _busy = false;
        _status = ok ? '完成' : '失败';
        _progress = ok ? 1.0 : _progress;
      });
      _addLog(ok ? '✔ 全部完成！便携应用在：$_builtDir' : '✖ 没能完成，请看上方的红色说明', error: !ok, success: ok);
      if (ok) {
        if (Settings.openWhenDone && _builtDir != null) {
          Process.run('explorer', [_builtDir!]);
        }
        if (Settings.runWhenDone && _builtExe != null) {
          Process.start(_builtExe!, [], runInShell: false);
        }
      }
      _scrollToBottom();
    } catch (e) {
      setState(() => _busy = false);
      _addLog('启动 pxe.exe 失败：${friendly(e.toString())}', error: true);
    }
  }

  void _onLine(String line, {bool stderr = false}) {
    final t = line.trim();
    final m = RegExp(r'^\[pxe\|progress\|([\d.]+)\|(.*)\]$').firstMatch(t);
    if (m != null) {
      setState(() {
        _progress = double.tryParse(m.group(1)!) ?? _progress;
        _status = m.group(2)!;
      });
      return;
    }
    if (t.startsWith('[pxe] portable app:')) _builtDir = t.substring('[pxe] portable app:'.length).trim();
    if (t.startsWith('[pxe] native stub launcher:')) _builtExe = t.substring('[pxe] native stub launcher:'.length).trim();
    if (t.isEmpty) return;
    final isError = stderr || t.startsWith('✖') || t.toLowerCase().contains('error') || t.toLowerCase().contains('failed');
    _addLog(isError ? '✖ ${friendly(t)}' : t, error: isError);
    _scrollToBottom();
  }

  void _addLog(String text, {bool error = false, bool success = false}) {
    setState(() => _log.add(_LogLine(text, error: error, success: success)));
  }

  void _scrollToBottom() {
    WidgetsBinding.instance.addPostFrameCallback((_) {
      if (_logCtrl.hasClients) {
        _logCtrl.animateTo(_logCtrl.position.maxScrollExtent,
            duration: const Duration(milliseconds: 150), curve: Curves.easeOut);
      }
    });
  }

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      body: Center(
        child: ConstrainedBox(
          constraints: const BoxConstraints(maxWidth: 680),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 28, vertical: 20),
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                // ------- 头部 -------
                Row(
                  children: [
                    Container(
                      width: 44, height: 44,
                      decoration: BoxDecoration(
                        gradient: const LinearGradient(colors: [Color(0xFF4DA3FF), Color(0xFF2D6BD4)]),
                        borderRadius: BorderRadius.circular(12),
                      ),
                      child: const Icon(Icons.bolt, color: Colors.white, size: 26),
                    ),
                    const SizedBox(width: 14),
                    Expanded(
                      child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                        Text('PortableXE', style: theme.textTheme.titleLarge?.copyWith(fontWeight: FontWeight.w700)),
                        Text('把 Windows 安装包变成便携应用 · 支持 MSI / NSIS / Inno / 7z / ZIP / RAR / CAB / TAR',
                            style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
                      ]),
                    ),
                    IconButton(
                      tooltip: '设置',
                      onPressed: _busy ? null : () => _showSettings(),
                      icon: const Icon(Icons.settings_outlined),
                    ),
                  ],
                ),
                const SizedBox(height: 20),

                Expanded(
                  child: SingleChildScrollView(
                    child: Column(
                      crossAxisAlignment: CrossAxisAlignment.start,
                      children: [
                        // ------- 第 1 步 -------
                        _card(theme, icon: Icons.download_outlined, title: '选择安装包', child: Column(children: [
                          _Field(
                            label: '文件',
                            controller: _inputCtrl,
                            hint: '点击右侧浏览选择 .exe / .msi / .7z / .zip / .rar / .cab / .tar.gz…',
                            action: OutlinedButton.icon(
                              onPressed: _busy ? null : _pickFile,
                              icon: const Icon(Icons.file_open_outlined, size: 18),
                              label: const Text('浏览'),
                            ),
                          ),
                          const SizedBox(height: 12),
                          _Field(label: '应用名称', controller: _nameCtrl, hint: '便携应用的文件夹名，自动从文件名提取'),
                        ])),
                        const SizedBox(height: 14),

                        // ------- 第 2 步 -------
                        _card(theme, icon: Icons.folder_copy_outlined, title: '输出与打包方式', child: Column(children: [
                          _Field(
                            label: '输出到',
                            controller: _outCtrl,
                            onChanged: () => _outTouched = true,
                            action: OutlinedButton.icon(
                              onPressed: _busy ? null : _pickDir,
                              icon: const Icon(Icons.folder_outlined, size: 18),
                              label: const Text('浏览'),
                            ),
                          ),
                          const SizedBox(height: 14),
                          Row(children: [
                            Text('打包为', style: theme.textTheme.bodyMedium?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
                            const SizedBox(width: 12),
                            Expanded(
                              child: SegmentedButton<PackKind>(
                                segments: [
                                  ButtonSegment(value: PackKind.dir, label: const Text('文件夹'), icon: const Icon(Icons.folder, size: 18)),
                                  ButtonSegment(value: PackKind.zip, label: const Text('ZIP'), icon: const Icon(Icons.archive_outlined, size: 18)),
                                  ButtonSegment(value: PackKind.sevenZip, label: const Text('7z'), icon: const Icon(Icons.inventory_2_outlined, size: 18)),
                                ],
                                selected: {_pack},
                                onSelectionChanged: _busy ? null : (s) => setState(() => _pack = s.first),
                                showSelectedIcon: false,
                              ),
                            ),
                          ]),
                          const SizedBox(height: 8),
                          Text(_pack.hint,
                              style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
                        ])),
                        const SizedBox(height: 20),

                        // ------- 操作区 -------
                        Row(children: [
                          FilledButton.icon(
                            onPressed: _busy ? null : _start,
                            icon: _busy
                                ? const SizedBox(width: 16, height: 16, child: CircularProgressIndicator(strokeWidth: 2))
                                : const Icon(Icons.bolt, size: 20),
                            label: Text(_busy ? '正在处理…' : '创建便携应用'),
                            style: FilledButton.styleFrom(
                              minimumSize: const Size(190, 46),
                              textStyle: const TextStyle(fontSize: 15, fontWeight: FontWeight.w600),
                            ),
                          ),
                          const SizedBox(width: 18),
                          Expanded(
                            child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                              ClipRRect(
                                borderRadius: BorderRadius.circular(4),
                                child: TweenAnimationBuilder<double>(
                                  tween: Tween(begin: 0, end: _progress),
                                  duration: const Duration(milliseconds: 250),
                                  builder: (_, v, __) => LinearProgressIndicator(
                                    value: v > 0 ? v : null,
                                    minHeight: 8,
                                    backgroundColor: theme.colorScheme.surfaceContainerHighest,
                                  ),
                                ),
                              ),
                              const SizedBox(height: 6),
                              Row(children: [
                                if (_busy)
                                  Container(
                                    padding: const EdgeInsets.symmetric(horizontal: 8, vertical: 1),
                                    decoration: BoxDecoration(
                                      color: theme.colorScheme.primary.withValues(alpha: 0.15),
                                      borderRadius: BorderRadius.circular(99),
                                    ),
                                    child: Text('进行中',
                                        style: TextStyle(fontSize: 11, color: theme.colorScheme.primary)),
                                  ),
                                const SizedBox(width: 4),
                                Expanded(
                                  child: Text(
                                    _progress > 0 && _progress < 1
                                        ? '${(_progress * 100).round()}% · $_status'
                                        : _status,
                                    style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant),
                                    overflow: TextOverflow.ellipsis,
                                  ),
                                ),
                              ]),
                            ]),
                          ),
                        ]),
                      ],
                    ),
                  ),
                ),

                // ------- 日志区 -------
                AnimatedCrossFade(
                  duration: const Duration(milliseconds: 200),
                  crossFadeState: _logOpen ? CrossFadeState.showSecond : CrossFadeState.showFirst,
                  secondChild: Container(
                    height: 170,
                    width: double.infinity,
                    margin: const EdgeInsets.only(top: 14),
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
                        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
                          for (final l in _log)
                            Padding(
                              padding: const EdgeInsets.only(bottom: 3),
                              child: Text(
                                l.text,
                                style: TextStyle(
                                  fontFamily: 'Consolas',
                                  fontSize: 12,
                                  height: 1.35,
                                  color: l.error
                                      ? theme.colorScheme.error
                                      : l.success
                                          ? const Color(0xFF9FE29F)
                                          : const Color(0xFFB8BCC6),
                                ),
                              ),
                            ),
                        ]),
                      ),
                    ),
                  ),
                  firstChild: const SizedBox(width: double.infinity, height: 0),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }

  Widget _card(ThemeData theme, {required IconData icon, required String title, required Widget child}) {
    return Card(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(18, 14, 18, 18),
        child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
          Row(children: [
            Icon(icon, size: 17, color: theme.colorScheme.primary),
            const SizedBox(width: 8),
            Text(title, style: theme.textTheme.labelLarge?.copyWith(fontWeight: FontWeight.w600)),
          ]),
          const SizedBox(height: 14),
          child,
        ]),
      ),
    );
  }

  Future<void> _showSettings() async {
    await showModalBottomSheet(
      context: context,
      showDragHandle: true,
      isScrollControlled: true,
      builder: (ctx) => const _SettingsSheet(),
    );
    setState(() {}); // 回主界面刷新
  }
}

// ---------------- 设置面板 ----------------

class _SettingsSheet extends StatefulWidget {
  const _SettingsSheet();

  @override
  State<_SettingsSheet> createState() => _SettingsSheetState();
}

class _SettingsSheetState extends State<_SettingsSheet> {
  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return SafeArea(
      child: Padding(
        padding: const EdgeInsets.fromLTRB(24, 0, 24, 24),
        child: Column(
          mainAxisSize: MainAxisSize.min,
          crossAxisAlignment: CrossAxisAlignment.start,
          children: [
            Text('设置', style: theme.textTheme.titleMedium?.copyWith(fontWeight: FontWeight.w700)),
            const SizedBox(height: 18),

            // 压缩级别
            Text('压缩级别：${Settings.compression}', style: theme.textTheme.labelLarge),
            Text(Settings.compression <= 3
                ? '快，但包大（适合大文件）'
                : Settings.compression >= 7
                    ? '包最小，但明显更慢'
                    : '速度与体积的平衡（推荐）',
                style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
            Slider(
              value: Settings.compression.toDouble(),
              min: 1, max: 9, divisions: 8,
              label: '${Settings.compression}',
              onChanged: (v) => setState(() => Settings.compression = v.round()),
            ),

            // 超时
            Text('解包超时：${Settings.timeoutMin} 分钟', style: theme.textTheme.labelLarge),
            Text('超过这个时间还没解完就放弃（大文件建议调大）',
                style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
            Slider(
              value: Settings.timeoutMin.toDouble(),
              min: 1, max: 60, divisions: 59,
              label: '${Settings.timeoutMin} 分钟',
              onChanged: (v) => setState(() => Settings.timeoutMin = v.round()),
            ),
            const Divider(height: 24),

            _switch(theme, '使用原生启动器（推荐）',
                '生成单个 .exe 启动器，无黑窗；关闭则生成 .bat（可手动改启动参数）',
                Settings.nativeStub, (v) => Settings.nativeStub = v),
            _switch(theme, '附带 VC++ 运行库说明',
                '在 Redist/ 文件夹里放一份 vc_redist 下载说明，部分软件需要它',
                Settings.redistNote, (v) => Settings.redistNote = v),
            const Divider(height: 24),

            _switch(theme, '完成后打开输出文件夹',
                '构建成功自动在资源管理器里打开结果目录',
                Settings.openWhenDone, (v) => Settings.openWhenDone = v),
            _switch(theme, '完成后启动应用',
                '构建成功自动运行生成的便携应用，方便立刻验证',
                Settings.runWhenDone, (v) => Settings.runWhenDone = v),
            _switch(theme, '覆盖已有的同名应用',
                '输出目录已有同名文件夹时直接替换；关闭则会提示报错',
                Settings.overwrite, (v) => Settings.overwrite = v),

            const SizedBox(height: 16),
            SizedBox(
              width: double.infinity,
              child: FilledButton(
                onPressed: () async {
                  await Settings.save();
                  if (context.mounted) Navigator.pop(context);
                },
                child: const Text('保存'),
              ),
            ),
          ],
        ),
      ),
    );
  }

  Widget _switch(ThemeData theme, String title, String desc, bool value, ValueChanged<bool> on) {
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(children: [
        Expanded(
          child: Column(crossAxisAlignment: CrossAxisAlignment.start, children: [
            Text(title, style: theme.textTheme.bodyMedium),
            Text(desc, style: theme.textTheme.bodySmall?.copyWith(color: theme.colorScheme.onSurfaceVariant)),
          ]),
        ),
        Switch(value: value, onChanged: (v) => setState(() => on(v))),
      ]),
    );
  }
}

// ---------------- 输入行 ----------------

class _Field extends StatelessWidget {
  final String label;
  final TextEditingController controller;
  final String? hint;
  final Widget? action;
  final VoidCallback? onChanged;

  const _Field({required this.label, required this.controller, this.hint, this.action, this.onChanged});

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.center,
      children: [
        SizedBox(width: 74, child: Text(label, style: Theme.of(context).textTheme.bodyMedium)),
        const SizedBox(width: 12),
        Expanded(
          child: TextField(
            controller: controller,
            style: const TextStyle(fontSize: 14),
            onChanged: onChanged != null ? (_) => onChanged!() : null,
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
