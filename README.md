# cleat-fgo-helper

FGO 日服 APK 构建和汉化资源工具，使用 Rust 实现。下载、解码、版本快照、剧情对齐、本地化合并及资源清单由工具自动处理。

## 构建资源包

安装稳定版 Rust 后编译：

```sh
cargo build --release --locked
```

在工作目录执行：

```sh
fgo-helper resources build
```

工具自动获取日服和国服当前资源，按当前版本的 ScriptEncryptSetting 选择解码方式，校验下载缓存，生成完整快照，再对齐剧情及本地化文本。不需要从模块导出 keys.json，也不用手动填写快照目录或先合并 Localization 文件。

输出位于 `dist/resources/<内容版本>/`：

- `stories.script`：通过对齐检查的中文剧情。
- `LocalizationJpn.txt`：保留日服键名与格式参数的本地化文本。
- `manifest.json`：版本、下载地址、大小与 SHA-256。
- `report.json`：缺少国服对应文本及需要人工核对的剧情列表。

`dist/resources/latest.txt` 指向最新构建。`--offline` 复用已校验快照，`--font <文件>` 可附加含 FGO-Main-Font-Mod 的 Unity Font AssetBundle。`--base-url` 可指定其他 HTTPS 资源目录，路径末尾必须有 `/`，允许使用 `{version}` 占位符。

对齐保留日服控制命令，只替换能确认对应关系的对白、选项和姓名。结构不匹配时记入报告，不覆盖游戏原文。下载失败、密钥索引缺失、损坏内容或不完整快照不会成为活动版本。工具支持真实资源中存在的 Unicode 空白、缺失/重复逗号及多行字符串，但仍校验解码和占位符。

## 发布到同仓库 Release

模块默认从 `jhq223/cleat-fgo` 的 `resources` Release 获取 manifest.json。首次创建：

```sh
gh release create resources --repo jhq223/cleat-fgo --title "Translation resources" --notes "中文剧情与本地化资源" --latest=false
```

在当前资源输出目录中发布，先传资源，再传清单：

```sh
gh release upload resources stories.script LocalizationJpn.txt --repo jhq223/cleat-fgo --clobber
gh release upload resources manifest.json --repo jhq223/cleat-fgo --clobber
```

包含字体时在第一条命令加上 `Font`。report.json 供维护者本地审核，不是模块必需文件。不要将资源 Release 标记为 latest，以免 APK 构建任务把它当成模块库版本。更新中读到不匹配的旧清单或缓存文件时，模块会校验失败并保留旧包，稍后重试。

剧情、游戏资产、账号导出和签名文件不应提交到源码仓库。资源发布者负责确认所发布素材的使用权限。

## 翻译维护工具

```sh
fgo-helper script export
fgo-helper script import
fgo-helper tools compare
fgo-helper tools deharmonize
fgo-helper tools localize
```

省略输入路径时自动读取当前已校验的日服/国服快照。仍可显式指定 `--input`、`--original`、`--jp` 或 `--cn`，用于人工翻译和调试。`script merge` 合并人工翻译 JSON；`scripts txt-to-bundle` / `bundle-to-txt` 用于单独打包和解包。

`res download jp`、`res download cn`、`res info jp`、`res list jp` 用于单个服务器数据操作。`mappings download` 获取 Chaldea 映射。所有命令均可用 `--help` 查看参数。已校验数据保存在 data/，工作目录决定缓存位置。

## APK 构建

需要 Java、apktool 和 apksigner：

```sh
fgo-helper apk setup --xapk game.xapk
fgo-helper apk build --ks /path/to/key.keystore --ks-pass PASSWORD --ks-alias ALIAS --lib-dir lib/
```

lib/ 直接包含待注入的 ARM64 `.so` 文件。自动构建与更新清单见 [cleat-fgo-build](https://github.com/jhq223/cleat-fgo-build)。签名材料保存在本地或 CI Secrets；不要提交仓库。

## 开发检查

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```

单元测试覆盖解码、损坏缓存、填充校验、脚本对齐和本地化格式；真实资源检查还需要运行 resources build。Unity 资产测试依赖本地下载文件，默认忽略。