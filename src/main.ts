import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import "./styles.css";

type ModelFile = { path: string; name: string };
type TextureChange = {
  file: string;
  model: string;
  old_path: string;
  new_path: string;
  filtered: boolean;
};
type PreviewResult = { changes: TextureChange[]; warnings: string[] };
type ApplyResult = { changed_files: number; changes: TextureChange[]; warnings: string[] };
type Options = {
  prefix: string;
  append_slash: boolean;
  skip_builtin: boolean;
  auto_backup: boolean;
  trim_resource: boolean;
  reset: boolean;
};
type SavedSettings = {
  prefix?: string;
  appendSlash?: boolean;
  skipBuiltin?: boolean;
  trimResource?: boolean;
  autoBackup?: boolean;
  showFullPath?: boolean;
  columnWidths?: Record<string, number[]>;
};

const app = document.querySelector<HTMLDivElement>("#app")!;
const SETTINGS_KEY = "xy-model-texture-path-settings-v1";

app.innerHTML = [
  '<main class="app-shell">',
  '  <section class="topbar">',
  '    <div class="title-block">',
  '      <h1>XY快速修改模型贴图路径</h1>',
  '      <p>导入 Warcraft III .mdx / .mdl 模型，预览并批量修改贴图引用路径。</p>',
  '    </div>',
  '    <div class="top-actions">',
  '      <button id="btn-open-files" class="secondary">选文件</button>',
  '      <button id="btn-open-folder" class="secondary">选文件夹</button>',
  '    </div>',
  '  </section>',
  '  <section id="drop-zone" class="drop-zone">',
  '    <span>拖入 .mdx / .mdl 或整个文件夹</span>',
  '    <input id="path-input" placeholder="也可以粘贴文件 / 文件夹路径，多个路径用英文分号 ; 分隔" />',
  '    <button id="btn-import" class="primary">导入路径</button>',
  '  </section>',
  '  <section class="rules-panel">',
  '    <label class="field prefix-field"><span>自定义路径</span><input id="prefix-input" placeholder="留空=只保留贴图名；例：war3mapImported 或 ($fileName)" /></label>',
  '    <label><input id="append-slash" type="checkbox" checked /> 路径后面自动补 &#92;</label>',
  '    <label><input id="skip-builtin" type="checkbox" checked /> 过滤魔兽内置贴图</label>',
  '    <label><input id="trim-resource" type="checkbox" checked /> 自动修改 resource 内模型贴图</label>',
  '    <label><input id="auto-backup" type="checkbox" /> 修改前自动备份</label>',
  '  </section>',
  '  <section class="workspace-grid">',
  '    <section class="panel model-panel">',
  '      <div class="panel-head"><h2>导入模型</h2><div class="panel-actions"><label class="compact-check"><input id="show-full-path" type="checkbox" /> 显示模型完整路径</label><button id="btn-clear-models" class="ghost">清空模型</button></div></div>',
  '      <div class="table-wrap"><table id="models-table" class="resizable-table"><colgroup><col style="width:72px" /><col /></colgroup><thead><tr><th>编号<span class="column-resizer" data-table="models-table" data-col="0"></span></th><th>模型路径</th></tr></thead><tbody id="models-body"><tr class="empty-row"><td colspan="2">暂无模型，拖入或选择模型文件。</td></tr></tbody></table></div>',
  '    </section>',
  '    <section class="panel preview-panel">',
  '      <div class="panel-head"><h2>贴图路径预览</h2><div class="preview-actions"><button id="btn-mod-preview" class="secondary">修改预览</button><button id="btn-reset-preview" class="secondary">重置预览</button></div></div>',
  '      <div class="table-wrap"><table id="preview-table" class="resizable-table"><colgroup><col style="width:32%" /><col style="width:34%" /><col style="width:34%" /></colgroup><thead><tr><th>模型<span class="column-resizer" data-table="preview-table" data-col="0"></span></th><th>原贴图路径<span class="column-resizer" data-table="preview-table" data-col="1"></span></th><th>新贴图路径</th></tr></thead><tbody id="preview-body"><tr class="empty-row"><td colspan="3">暂无可预览的贴图路径。</td></tr></tbody></table></div>',
  '    </section>',
  '  </section>',
  '  <section class="commit-bar">',
  '    <button id="btn-modify" class="danger">修改路径</button>',
  '    <button id="btn-reset" class="danger-outline">重置路径</button>',
  '    <button id="btn-clear-all" class="ghost">清空所有</button>',
  '  </section>',
  '  <section class="panel log-panel">',
  '    <div class="panel-head"><h2>贴图修改日志</h2><button id="btn-clear-log" class="ghost">清空日志</button></div>',
  '    <textarea id="log" readonly></textarea>',
  '  </section>',
  '</main>'
].join("");

const pathInput = byId<HTMLInputElement>("path-input");
const prefixInput = byId<HTMLInputElement>("prefix-input");
const appendSlash = byId<HTMLInputElement>("append-slash");
const skipBuiltin = byId<HTMLInputElement>("skip-builtin");
const trimResource = byId<HTMLInputElement>("trim-resource");
const autoBackup = byId<HTMLInputElement>("auto-backup");
const showFullPath = byId<HTMLInputElement>("show-full-path");
const modelsBody = byId<HTMLTableSectionElement>("models-body");
const previewBody = byId<HTMLTableSectionElement>("preview-body");
const logBox = byId<HTMLTextAreaElement>("log");

let models: ModelFile[] = [];

function byId<T extends HTMLElement>(id: string): T {
  return document.getElementById(id) as T;
}

function currentOptions(reset: boolean): Options {
  return {
    prefix: reset ? "" : prefixInput.value,
    append_slash: reset ? false : appendSlash.checked,
    skip_builtin: skipBuiltin.checked,
    auto_backup: autoBackup.checked,
    trim_resource: trimResource.checked,
    reset: reset
  };
}

function writeLog(message: string) {
  const time = new Date().toLocaleTimeString("zh-CN", { hour12: false });
  logBox.value += time + "  " + message + "\n";
  logBox.scrollTop = logBox.scrollHeight;
}

function tableColumnWidths(tableId: string): number[] {
  const table = document.getElementById(tableId) as HTMLTableElement | null;
  if (!table) return [];
  return Array.from(table.querySelectorAll<HTMLTableCellElement>("thead th"))
    .map((cell) => Math.round(cell.getBoundingClientRect().width));
}

function applyTableColumnWidths(tableId: string, widths?: number[]) {
  if (!widths || widths.length === 0) return;
  const table = document.getElementById(tableId) as HTMLTableElement | null;
  if (!table) return;
  const columns = Array.from(table.querySelectorAll<HTMLTableColElement>("col"));
  if (columns.length !== widths.length) return;
  widths.forEach((width, index) => {
    if (Number.isFinite(width) && width > 0) {
      columns[index].style.width = width + "px";
    }
  });
}

function saveSettings() {
  const settings: SavedSettings = {
    prefix: prefixInput.value,
    appendSlash: appendSlash.checked,
    skipBuiltin: skipBuiltin.checked,
    trimResource: trimResource.checked,
    autoBackup: autoBackup.checked,
    showFullPath: showFullPath.checked,
    columnWidths: {
      "models-table": tableColumnWidths("models-table"),
      "preview-table": tableColumnWidths("preview-table")
    }
  };
  localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
}

function loadSettings() {
  try {
    const raw = localStorage.getItem(SETTINGS_KEY);
    if (!raw) return;
    const settings = JSON.parse(raw) as SavedSettings;
    if (typeof settings.prefix === "string") prefixInput.value = settings.prefix;
    if (typeof settings.appendSlash === "boolean") appendSlash.checked = settings.appendSlash;
    if (typeof settings.skipBuiltin === "boolean") skipBuiltin.checked = settings.skipBuiltin;
    if (typeof settings.trimResource === "boolean") trimResource.checked = settings.trimResource;
    if (typeof settings.autoBackup === "boolean") autoBackup.checked = settings.autoBackup;
    if (typeof settings.showFullPath === "boolean") showFullPath.checked = settings.showFullPath;
    requestAnimationFrame(() => {
      applyTableColumnWidths("models-table", settings.columnWidths?.["models-table"]);
      applyTableColumnWidths("preview-table", settings.columnWidths?.["preview-table"]);
    });
  } catch (error) {
    console.warn("读取保存设置失败", error);
  }
}

function setupColumnResizers() {
  document.querySelectorAll<HTMLElement>(".column-resizer").forEach((handle) => {
    handle.addEventListener("pointerdown", (event) => {
      const tableId = handle.dataset.table;
      const columnIndex = Number(handle.dataset.col);
      if (!tableId || Number.isNaN(columnIndex)) return;

      const table = document.getElementById(tableId) as HTMLTableElement | null;
      if (!table) return;
      const columns = Array.from(table.querySelectorAll<HTMLTableColElement>("col"));
      const headers = Array.from(table.querySelectorAll<HTMLTableCellElement>("thead th"));
      const leftColumn = columns[columnIndex];
      const rightColumn = columns[columnIndex + 1];
      if (!leftColumn || !rightColumn || headers.length !== columns.length) return;

      headers.forEach((header, index) => {
        columns[index].style.width = header.getBoundingClientRect().width + "px";
      });

      const startX = event.clientX;
      const leftStart = headers[columnIndex].getBoundingClientRect().width;
      const rightStart = headers[columnIndex + 1].getBoundingClientRect().width;
      const leftMinimum = tableId === "models-table" && columnIndex === 0 ? 56 : 110;
      const rightMinimum = 120;

      handle.setPointerCapture(event.pointerId);
      document.body.classList.add("resizing-columns");
      event.preventDefault();

      const move = (moveEvent: PointerEvent) => {
        const rawDelta = moveEvent.clientX - startX;
        const delta = Math.max(
          leftMinimum - leftStart,
          Math.min(rightStart - rightMinimum, rawDelta)
        );
        leftColumn.style.width = leftStart + delta + "px";
        rightColumn.style.width = rightStart - delta + "px";
      };

      const stop = () => {
        document.body.classList.remove("resizing-columns");
        window.removeEventListener("pointermove", move);
        window.removeEventListener("pointerup", stop);
        window.removeEventListener("pointercancel", stop);
        saveSettings();
      };

      window.addEventListener("pointermove", move);
      window.addEventListener("pointerup", stop);
      window.addEventListener("pointercancel", stop);
    });
  });
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (ch) => ({
    "&": "&amp;",
    "<": "&lt;",
    ">": "&gt;",
    '"': "&quot;",
    "'": "&#39;"
  }[ch] || ch));
}

function renderModels() {
  if (models.length === 0) {
    modelsBody.innerHTML = '<tr class="empty-row"><td colspan="2">暂无模型，拖入或选择模型文件。</td></tr>';
    return;
  }
  modelsBody.innerHTML = models.map((model, index) =>
    '<tr title="' + escapeHtml(model.path) + '">' +
    '<td class="col-index">' + (index + 1) + '</td>' +
    '<td>' + escapeHtml(showFullPath.checked ? model.path : model.name) + '</td>' +
    '</tr>'
  ).join("");
}

function renderPreview(changes: TextureChange[]) {
  if (changes.length === 0) {
    previewBody.innerHTML = '<tr class="empty-row"><td colspan="3">暂无可预览的贴图路径。</td></tr>';
    return;
  }
  previewBody.innerHTML = changes.map((change) =>
    '<tr class="' + (change.filtered ? "filtered-row" : "") + '" title="' + escapeHtml(change.file) + '">' +
    '<td>' + escapeHtml(change.model) + '</td>' +
    '<td>' + escapeHtml(change.old_path) + '</td>' +
    '<td>' + escapeHtml(change.new_path) + '</td>' +
    '</tr>'
  ).join("");
}

async function addPaths(paths: string[]) {
  const clean = paths.map((item) => item.trim()).filter(Boolean);
  if (clean.length === 0) return;
  const found = await invoke<ModelFile[]>("collect_models", { paths: clean });
  const existed = new Set(models.map((model) => model.path.toLowerCase()));
  let added = 0;
  for (const model of found) {
    const key = model.path.toLowerCase();
    if (!existed.has(key)) {
      models.push(model);
      existed.add(key);
      added++;
    }
  }
  renderModels();
  writeLog("已加入 " + added + " 个模型文件");
}

async function importFromInput() {
  const raw = pathInput.value.trim();
  if (!raw) {
    writeLog("请输入模型文件/文件夹路径，或直接拖入文件。");
    return;
  }
  await addPaths(raw.split(";"));
}

async function runPreview(reset: boolean) {
  if (models.length === 0) {
    writeLog("请先导入模型。");
    return;
  }
  try {
    const result = await invoke<PreviewResult>("preview_paths", {
      files: models.map((model) => model.path),
      opts: currentOptions(reset)
    });
    renderPreview(result.changes);
    result.warnings.forEach((item) => writeLog("警告：" + item));
    if (result.changes.length === 0) {
      writeLog("未解析到贴图路径，请检查模型格式或模型是否包含贴图。");
    } else {
      writeLog((reset ? "重置预览" : "修改预览") + "：" + result.changes.length + " 条贴图路径");
    }
  } catch (error) {
    writeLog((reset ? "重置预览失败：" : "修改预览失败：") + String(error));
  }
}

async function applyChanges(reset: boolean) {
  if (models.length === 0) {
    writeLog("请先导入模型。");
    return;
  }
  try {
    const result = await invoke<ApplyResult>("apply_paths", {
      files: models.map((model) => model.path),
      opts: currentOptions(reset)
    });
    renderPreview(result.changes);
    result.warnings.forEach((item) => writeLog("警告：" + item));
    writeLog((reset ? "重置完成" : "完成") + "。修改文件数：" + result.changed_files);
  } catch (error) {
    writeLog((reset ? "重置失败：" : "修改失败：") + String(error));
  }
}

async function chooseFiles() {
  const selected = await open({
    multiple: true,
    directory: false,
    filters: [{ name: "War3 模型", extensions: ["mdx", "mdl"] }]
  });
  if (Array.isArray(selected)) await addPaths(selected);
  else if (typeof selected === "string") await addPaths([selected]);
}

async function chooseFolder() {
  const selected = await open({ multiple: false, directory: true });
  if (typeof selected === "string") await addPaths([selected]);
}

byId("btn-open-files").addEventListener("click", chooseFiles);
byId("btn-open-folder").addEventListener("click", chooseFolder);
byId("btn-import").addEventListener("click", importFromInput);
byId("btn-mod-preview").addEventListener("click", () => runPreview(false));
byId("btn-reset-preview").addEventListener("click", () => runPreview(true));
byId("btn-modify").addEventListener("click", () => applyChanges(false));
byId("btn-reset").addEventListener("click", () => applyChanges(true));
byId("btn-clear-models").addEventListener("click", () => {
  models = [];
  renderModels();
  renderPreview([]);
  writeLog("已清空模型");
});
byId("btn-clear-log").addEventListener("click", () => { logBox.value = ""; });
prefixInput.addEventListener("input", saveSettings);
[appendSlash, skipBuiltin, trimResource, autoBackup].forEach((checkbox) => {
  checkbox.addEventListener("change", saveSettings);
});
showFullPath.addEventListener("change", () => {
  renderModels();
  saveSettings();
});
byId("btn-clear-all").addEventListener("click", () => {
  models = [];
  renderModels();
  renderPreview([]);
  logBox.value = "";
});
pathInput.addEventListener("keydown", (event) => {
  if (event.key === "Enter") void importFromInput();
});

const appWindow = getCurrentWindow();
void appWindow.onDragDropEvent((event) => {
  if (event.payload.type === "drop") {
    void addPaths(event.payload.paths);
  }
});

loadSettings();
setupColumnResizers();
