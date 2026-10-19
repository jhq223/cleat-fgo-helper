# cleat-fgo-helper

FGO 日服汉化资源和 APK 构建工具。

[汉化模块](https://github.com/jhq223/cleat-fgo) · [APK 构建](https://github.com/jhq223/cleat-fgo-build) · [修改历史](CHANGELOG.md)

## 安装

Windows 用户从 [Releases](https://github.com/jhq223/cleat-fgo-helper/releases/latest) 下载 `fgo-helper-win.zip`，解压后使用 `fgo-helper.exe`。

从源码安装：

```sh
cargo install --path . --locked
```

## 生成汉化资源

```sh
fgo-helper resources build --font resources/Font
```

将字体 AssetBundle 保存为 `resources/Font`，也可从模块的 [resources Release](https://github.com/jhq223/cleat-fgo/releases/tag/resources) 下载 `Font`。

工具下载日服和国服当前资源，生成包含剧情、界面文本和字体的更新包。输出目录为 `dist/resources/<版本>/`，版本号写入 `dist/resources/latest.txt`。

| 文件 | 用途 |
|---|---|
| `stories.script` | 中文剧情包 |
| `LocalizationJpn.txt` | 界面本地化文本 |
| `Font` | 中文字体 AssetBundle |
| `manifest.json` | 供模块检查更新的资源清单 |
| `report.json` | 待补充和待核对的剧情列表 |

剧情包保留日服控制指令，替换对应的对白、选项和姓名。构建后查看 `report.json`，其中 `missing_cn` 列出待补充国服文本的脚本，`needs_review` 列出需要人工对齐的脚本及原因。

| 选项 | 用途 |
|---|---|
| `--offline` | 使用已下载的数据构建 |
| `--font <文件>` | 附加包含 `FGO-Main-Font-Mod` 的 Unity 字体 AssetBundle |
| `--output <目录>` | 输出目录，默认为 `dist/resources` |
| `--base-url <URL>` | 发布资源的 HTTPS 目录，末尾带 `/`，可使用 `{version}` 占位符 |

下载数据保存在工作目录的 `data/` 下。

## 发布资源

模块默认读取 `jhq223/cleat-fgo` 的 `resources` Release。使用 GitHub CLI 首次创建：

```sh
gh release create resources --repo jhq223/cleat-fgo --title "Translation resources" --notes "中文剧情与本地化资源" --latest=false
```

进入本次构建的 `dist/resources/<版本>/` 目录，先上传资源文件，再上传清单：

```sh
gh release upload resources stories.script LocalizationJpn.txt Font --repo jhq223/cleat-fgo --clobber
gh release upload resources manifest.json --repo jhq223/cleat-fgo --clobber
```

模块会在下一次更新检查时下载新资源。

## 人工翻译

导出当前日服剧情的文本：

```sh
fgo-helper script export
```

编辑 `data/export/` 中 JSON 条目的 `message` 字段。`*_char.json` 对应姓名，其余文件对应对白和选项。完成后导入并打包：

```sh
fgo-helper script import
fgo-helper scripts txt-to-bundle --input data/output --output translated.script
```

将 `translated.script` 放入模块的 `Mod/overrides/` 目录。

其他维护命令：

| 命令 | 用途 |
|---|---|
| `tools compare` | 提取日服独有剧情到 `data/jp_only/` |
| `tools deharmonize` | 应用名称替换，输出到 `data/cn_deharmonized/` |
| `tools localize` | 单独合并界面本地化文件 |
| `mappings download` | 下载 Chaldea 映射到 `data/mappings/` |
| `res download jp` / `res download cn` | 更新指定服务器的数据 |
| `res info jp` / `res list jp` | 查看资源版本和文件列表 |

输入路径默认使用当前资源快照。每个命令的参数可通过 `--help` 查看。

## 构建 APK

准备 Java、apktool、apksigner、游戏 XAPK 和签名文件。将 ARM64 模块 `.so` 放入 `lib/`，执行：

```sh
fgo-helper apk setup --xapk game.xapk
fgo-helper apk build --ks /path/to/key.keystore --ks-pass PASSWORD --ks-alias ALIAS --lib-dir lib/
```

自动构建流程见 [cleat-fgo-build](https://github.com/jhq223/cleat-fgo-build)。

## 开发检查

```sh
cargo fmt --check
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
```
