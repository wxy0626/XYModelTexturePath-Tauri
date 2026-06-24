# XY快速修改模型贴图路径

一个基于 Tauri v2 的魔兽模型贴图路径批量修改工具。

## 功能

- 导入 `.mdx` / `.mdl` 模型并预览贴图路径
- 自定义贴图路径前缀，支持自动补 `\`
- 修改预览、重置预览、修改路径、重置路径
- 自动备份，备份文件名为 `原名称back次数.扩展名`
- 支持过滤魔兽内置贴图路径
- 支持自动修改 `resource` 内模型贴图路径
- 设置项本地保存
- 模型列表和贴图预览表格支持列宽拖动

## 开发运行

```bash
npm install
npm run tauri:dev
```

## 构建

```bash
npm run tauri:build
```

构建后的 Windows 程序会在 `src-tauri/target/release/` 下生成。
