# 前端模板开发规范

本文档约束本项目及后续基于此模板开发的前端代码。新增页面、组件和交互时，必须延续现有技术方案、Catppuccin 主题能力与界面语言，避免在同一项目中形成第二套设计系统。

## 1. 核心目标

- **一致性优先**：复用现有组件、布局、变量和交互模式，不为单个页面发明例外。
- **主题优先**：任何功能必须同时适配 Latte、Frappé、Macchiato、Mocha，不能只在当前主题下可用。
- **强调色可切换**：任何强调状态必须兼容全部 14 种 Catppuccin 强调色。
- **可访问性内建**：键盘操作、焦点状态、语义标签、对比度和减少动态效果不是后续补丁。
- **模板可维护**：优先使用简单、明确、可复用的实现；不得留下演示用占位逻辑、无效入口或重复规范。

## 2. 技术基线

- Vue 3，统一使用 `<script setup lang="ts">` 和 Composition API。
- TypeScript 必须保持严格类型；不得用 `any`、无意义类型断言或关闭类型检查规避问题。
- Pinia 只管理跨组件、需要持久化或具有明确业务边界的状态；局部 UI 状态保留在组件内。
- Vue Router 负责页面级导航，不使用手写 URL 状态代替路由。
- 图标只能使用 `vue-icons-plus`。
- 全局即时通知统一使用 `vue-sonner`；业务代码通过项目通知入口调用，不直接创建第二套 Toast 实现。
- 样式使用原生 CSS、CSS 自定义属性和现有样式文件，不新增平行的 CSS-in-JS 或主题框架。
- 包管理和脚本统一使用 Bun。

常用命令：

```sh
bun run dev
bun run format
bun run type-check
bun run build
```

提交前至少执行 `bun run format` 和 `bun run build`；`build` 已包含类型检查。

## 3. 目录与职责

当前核心文件：

- `src/assets/theme.css`：Catppuccin 原始色板、强调色映射、语义颜色变量和全局基础样式。
- `src/assets/app.css`：应用布局、组件外观、交互状态和响应式规则。
- `src/assets/sonner.css`：Vue Sonner 的 Catppuccin 变量映射、通知外观和响应式覆盖。
- `src/stores/theme.ts`：风味、强调色、持久化以及 `<html>` 主题属性。
- `src/App.vue`：当前应用外壳和主题设置示例。
- `src/router/`：页面路由。
- `src/stores/`：Pinia 状态。
- `src/composables/useAppToast.ts`：应用级通知类型、默认时长、去重和调用入口。

项目扩展后应按职责拆分：

```text
src/
├── assets/       # 全局主题和静态样式
├── components/   # 可复用界面组件
├── composables/  # 可复用组合式逻辑
├── router/       # 路由定义
├── stores/       # 跨组件状态
└── views/        # 路由级页面
```

规则：

- 路由级页面放入 `views/`，跨页面复用的界面放入 `components/`。
- 不创建只有一次调用、只转发参数或只返回一个表达式的工具函数。
- 不提前抽象；出现稳定、重复的行为后再提取组件或 composable。
- 组件名称使用 PascalCase，composable 使用 `useXxx`，Pinia store 使用 `useXxxStore`。
- Props、emits 和公开数据结构必须显式声明类型。
- 删除已失效的代码路径，不保留别名、兼容壳或注释掉的旧实现。

## 4. Catppuccin 主题规范

### 4.1 支持范围

必须支持四种官方风味：

| Flavor    | 类型 | 定位                   |
| --------- | ---- | ---------------------- |
| Latte     | 浅色 | 明亮、轻盈             |
| Frappé    | 深色 | 柔和、低对比           |
| Macchiato | 深色 | 平衡、沉静             |
| Mocha     | 深色 | 深邃、高对比；默认主题 |

必须支持 14 种强调色：Rosewater、Flamingo、Pink、Mauve、Red、Maroon、Peach、Yellow、Green、Teal、Sky、Sapphire、Blue、Lavender。默认强调色为 Mauve。

色值以 [Catppuccin 官方调色板](https://github.com/catppuccin/catppuccin) 为准，不凭视觉近似创建色值。

### 4.2 颜色变量分层

颜色分为三层：

1. **原始色板**：`--ctp-*`，只在 `src/assets/theme.css` 中定义。
2. **主题语义**：`--page`、`--panel`、`--card`、`--text`、`--border`、`--success` 等。
3. **组件局部变量**：例如 `--swatch`、`--item-color`、`--bar-size`，只表达组件内部变化。

组件样式必须优先使用语义变量：

```css
.example-card {
  color: var(--text);
  background: var(--card);
  border: 1px solid var(--border);
}
```

禁止在组件、页面或模板中直接写颜色字面量：

```css
/* 禁止 */
.example-card {
  color: #cdd6f4;
  background: rgb(30, 30, 46);
}
```

仅以下位置允许出现 Hex、RGB 或 HSL 色值：

- `src/assets/theme.css` 中四套官方 Catppuccin 原始色板定义。
- 浏览器或协议明确要求颜色字面量、且无法使用 CSS 变量的位置；必须附带原因说明。

### 4.3 原始色与语义色的使用边界

- 普通文本使用 `--text`、`--text-muted`、`--text-subtle`。
- 页面层级使用 `--page`、`--panel`、`--card`、`--card-raised`。
- 边框使用 `--border`、`--border-strong`。
- 操作强调使用 `--accent`；强调色文字使用 `--accent-readable`，尤其不能直接假设 Latte 下的强调色具有足够对比度。
- 状态使用 `--success`、`--warning`、`--danger`、`--info`。
- `--ctp-*` 只能用于色板展示、明确的 Catppuccin 状态映射或定义语义变量；常规组件不得绕过语义层。
- 透明衍生色使用 `color-mix(in srgb, var(...) ...%, transparent)`，不得手写带透明度的近似色。
- 不使用纯白、纯黑或项目外的任意品牌色破坏主题一致性。

### 4.4 主题状态约定

主题通过根元素属性控制：

```html
<html data-theme="mocha" data-accent="mauve"></html>
```

- `data-theme` 只能使用 `latte | frappe | macchiato | mocha`。
- `data-accent` 只能使用 `src/stores/theme.ts` 中声明的强调色。
- 主题状态统一由 `useThemeStore` 管理，组件不得自行直接修改 `document.documentElement.dataset`。
- 持久化键保持为 `catppuccin-flavor` 和 `catppuccin-accent`。
- Latte 必须设置 `color-scheme: light`，其他风味必须设置 `color-scheme: dark`。
- 修改风味或强调色列表时，必须同步更新类型、选项、CSS 变量映射和主题设置 UI。
- 不使用 `!important` 解决主题层叠问题；应修正变量层级、选择器作用域或声明顺序。

## 5. 界面设计语言

### 5.1 视觉基调

界面应体现 Catppuccin 的柔和、温暖和适中对比度，同时保持产品界面的清晰层级。

- 大面积背景使用 Base、Mantle、Crust 的语义映射。
- Surface 系列用于卡片、输入框、悬停和边框层级。
- 强调色用于选中、焦点、关键数据和少量装饰，不能铺满整个页面。
- 卡片通过背景层级和细边框区分；阴影只用于需要浮层感的重要容器。
- 圆角保持克制：普通控件约 8–13px，主要卡片约 15–18px，胶囊只用于标签或状态。
- 不允许 `color-mix()` 渐变、半透明和 `backdrop-filter` 构建氛围，但信息层不能依赖模糊效果才能辨认。
- 不使用高饱和霓虹色、重拟物、高亮玻璃边框、夸张渐变或无意义装饰动画。

### 5.2 信息层级

- 页面只保留一个主标题，标题应短、明确并形成视觉锚点。
- 辅助说明使用 `--text-muted`，元信息使用 `--text-subtle`。
- 一个卡片只表达一个主要目标；标题、主要数据、辅助状态按顺序组织。
- 空白优先于额外分割线。确需分隔时使用 `--border`。
- 关键动作、当前导航和选中状态必须明确，但不能同时出现多个竞争视觉焦点。
- 不使用纯颜色传递唯一信息；同时提供文字、图标、勾选、边框或 `aria-*` 状态。

### 5.3 间距与布局

- 使用稳定的 4px 间距节奏，常用间距为 8、12、16、20、24、32、40、48、64px。
- 页面内容设置合理最大宽度，宽屏下不得无限拉伸文本和卡片。
- 列表与表单优先使用 CSS Grid 或 Flexbox，不通过绝对定位拼接主体布局。
- 绝对定位只用于徽标、角标和不参与文档流的小型装饰。
- 不用固定高度包裹可变文本；应允许中文、英文和较长内容正常换行。

## 6. 响应式要求

现有断点是模板基线：

- `1050px`：收紧桌面布局和色板列数。
- `780px`：侧栏转为顶部主题设置，内容变为单列。
- `560px`：进入手机布局，减少边距和非关键操作。
- 最小支持宽度为 `320px`。

新增界面必须满足：

- 不出现横向页面滚动。
- 文本不被固定高度裁切。
- 触控目标足够点击，重要按钮建议至少约 38×38px。
- 桌面多列在窄屏下按信息优先级转为单列，不仅是等比例缩小。
- 非关键导航或装饰可以在手机端隐藏，但核心功能和状态必须保留。
- 至少在桌面宽度和 390px 手机宽度下进行浏览器验证。

## 7. 图标规范

所有界面图标必须来自 `vue-icons-plus`：

```vue
<script setup lang="ts">
import { LuSettings2 } from "vue-icons-plus/lu";
</script>

<template>
  <LuSettings2 :size="16" aria-hidden="true" />
</template>
```

要求：

- 从具体图标集子路径导入，例如 `vue-icons-plus/lu`，保持 tree-shaking。
- 禁止手写内联 `<svg>`、复制 SVG path、使用 icon font、Unicode 图形或 Emoji 代替产品图标。
- 同一区域优先使用同一图标集和一致的描边风格。
- 纯装饰图标设置 `aria-hidden="true"`。
- 仅图标按钮必须提供准确的 `aria-label` 和可见的 hover/focus 状态。
- 图标大小遵循所在控件层级，不通过图标替代文字解释复杂动作。

### 7.1 Vue Sonner 通知规范

全局即时反馈统一使用 `vue-sonner`。根应用只挂载一个 `<Toaster>`，业务组件通过 `useAppToast()` 调用通知，不得直接散落 `toast()` 参数或再引入其他 Toast 库。

基础结构：

```vue
<script setup lang="ts">
import { Toaster } from "vue-sonner";
import { LuCheckCircle, LuX } from "vue-icons-plus/lu";

import { useAppToast } from "@/composables/useAppToast";

const { notify } = useAppToast();
</script>

<template>
  <Toaster close-button close-button-position="top-right" rich-colors>
    <template #success-icon><LuCheckCircle aria-hidden="true" /></template>
    <template #close-icon><LuX aria-hidden="true" /></template>
  </Toaster>

  <button
    type="button"
    @click="notify('success', '设置已保存', '修改已立即生效。')"
  >
    保存
  </button>
</template>
```

要求：

- `<Toaster>` 只能在根应用外壳挂载一次，主题必须与当前 Catppuccin 风味同步：Latte 使用 `light`，其余风味使用 `dark`。
- 默认位置为右上角，移动端保持安全边距；同时最多展示 3 条，默认自动关闭时间为 4000ms。
- 通知统一分为 `success`、`info`、`warning`、`error`：成功表示操作完成，信息表示非阻塞提示，警告表示可恢复风险，错误表示操作失败。
- 相同类型与标题必须使用稳定 `id` 去重，避免快速点击生成通知队列。
- 通知标题应是一句明确结果，描述补充原因或下一步；禁止只写“成功”“错误”或暴露原始异常堆栈。
- Toast 只用于短暂的全局反馈。字段校验放在字段附近，长期系统状态留在页面内；常规关键确认使用对话框，只有统一封装、仅含“取消/确认”且关闭等价于取消的破坏性确认通知可以例外。
- 常规成功或信息通知自动关闭；需要用户读取或操作的错误不能只依赖短时 Toast，应在页面保留可恢复入口。
- `src/assets/sonner.css` 必须将 Sonner 背景、边框、文字和状态色映射到现有语义变量，不使用库的默认黑白色板或硬编码颜色。
- Success、Info、Warning、Error、Loading 和 Close 图标全部通过 `<Toaster>` slots 替换为 `vue-icons-plus`；不得使用 Sonner 内置 SVG 图标。
- 关闭按钮固定在通知右上角，并必须有可访问名称；通知容器必须有明确 `container-aria-label`，不得通过持续弹出通知打断屏幕阅读器用户。
- 除明确的撤销、重试或统一封装的破坏性确认外，不在 Toast 中放置操作；确认通知只能包含一个取消和一个确认按钮，不得包含表单或其他竞争操作。
- 不使用 `toast.custom()` 绕过设计系统；确需自定义内容时，必须复用语义变量、通知布局和可访问性约束。
- 修改通知封装或样式后，必须分别触发四种通知，并在 Latte 和至少一个暗色风味下进行浏览器验证。

## 8. 交互与动效

- 所有可点击元素使用原生 `<button>`、`<a>`、`<input>` 等语义元素，不用 `<div @click>` 模拟控件。
- 当前选项使用 `aria-pressed`、`aria-current` 或对应原生状态。
- 表单控件必须有可访问名称；可见标签优先，必要时使用 `aria-label`。
- 键盘焦点统一使用 `--focus-ring`，不得移除焦点轮廓后不提供替代方案。
- hover 只是增强，功能不能依赖 hover。
- 常规过渡控制在约 140–200ms，仅使用颜色、边框、透明度和轻微位移。
- 禁止为普通操作添加大幅缩放、弹跳或长时间动画。
- 必须保留 `prefers-reduced-motion: reduce` 支持。
- 主题切换应即时完成，不使用导致整页闪烁或文字不可读的转场。

## 9. 可访问性要求

- 正文和交互文字应达到 WCAG AA 对比度；浅色主题和 Yellow、Sky、Green 等强调色必须重点验证。
- 小字号文本不能仅靠低对比度表达层级；必要时增大字号、字重或使用 `--accent-readable`。
- 不以颜色作为成功、失败、选中或警告的唯一标识。
- DOM 阅读顺序必须与视觉顺序一致。
- 图片必须有合适的 `alt`；纯装饰图片使用空 `alt`。
- 图表应提供可访问名称或等价文本信息。
- 交互控件必须能通过键盘访问和操作。
- 主题切换后仍需验证焦点、选中态、边框和辅助文本清晰可见。

## 10. Vue 与组件实现要求

- 模板保持声明式；复杂数据变换放入 `computed`，副作用放入 `watch` 或生命周期函数。
- 不在模板中堆叠复杂条件和重复查找；重复逻辑应先形成明确的数据结构。
- `v-for` 必须使用稳定业务键，不能默认使用数组索引。
- 只在真正跨组件共享时引入 store；不要把所有状态集中到 Pinia。
- 直接修改 Pinia setup store 暴露的 ref 是允许的，但主题 DOM 副作用仍由 store 内部统一处理。
- 异步交互必须提供 loading、成功、空状态和真实错误状态；不得用静默失败伪装成功。
- 避免不必要的数组复制、深层响应式对象和重复计算。
- 不新增仅用于包装第三方组件、却没有增加行为或设计约束的组件。

## 11. 新增页面或组件的检查流程

### 开发前

1. 搜索是否已有相同组件、变量或交互模式。
2. 确定页面使用哪些现有语义变量，不先写固定颜色。
3. 明确桌面、平板和手机下的信息优先级。
4. 确认所需图标存在于 `vue-icons-plus`。
5. 确认即时反馈是否应使用 `useAppToast()`，不要把字段错误或长期状态错误地设计为 Toast。

### 开发中

1. 先完成真实功能和状态流，再做装饰。
2. 同时检查 Latte 和至少一个暗色风味。
3. 使用多种强调色测试，至少包含 Mauve、Yellow、Green、Blue。
4. 保持控件的默认、hover、focus、active、disabled 和错误状态一致。

### 完成前

1. 运行 `bun run format`。
2. 运行 `bun run build`，确保 TypeScript 和生产构建通过。
3. 在真实浏览器中操作变更路径，而不是只检查静态代码。
4. 验证四种风味与全部强调色的变量映射没有缺失。
5. 刷新页面，确认主题和强调色持久化。
6. 检查桌面和 390px 移动端，无横向溢出、遮挡、裁切和缺失图标。
7. 检查浏览器控制台无新增错误。
8. 分别触发 Success、Info、Warning、Error 通知，检查主题色、图标、关闭按钮、自动关闭和重复通知去重。
9. 删除临时截图、调试代码、占位数据和废弃样式。

## 12. 禁止事项

- 禁止在 `theme.css` 色板定义外硬编码界面颜色。
- 禁止绕过 `useThemeStore` 创建第二套主题状态。
- 禁止引入与 Catppuccin 并行的颜色系统或 UI 框架默认主题。
- 禁止手写 SVG 或使用 `vue-icons-plus` 以外的图标来源。
- 禁止在业务组件中直接引入 `toast`、新增其他 Toast 库或使用 `window.alert()` 代替应用通知。
- 禁止保留 Vue Sonner 内置 SVG 图标；所有通知类型及关闭图标必须由 `vue-icons-plus` 提供。
- 禁止只实现暗色或只实现浅色状态。
- 禁止用 `!important`、大范围全局选择器或脆弱的 DOM 层级选择器修补组件。
- 禁止没有键盘和焦点状态的可点击控件。
- 禁止为了视觉丰富而牺牲可读性、响应式或加载性能。
- 禁止提交未工作的按钮、假数据回退、空实现、`TODO: implement` 或演示脚手架。

## 13. 完成定义

一个前端变更只有同时满足以下条件才算完成：

- 功能路径在浏览器中真实可用。
- 四种 Catppuccin 风味下结构和内容均清晰。
- 全部强调色可用，且强调文本使用正确的可读色。
- 所有颜色来自 CSS 变量，所有图标来自 `vue-icons-plus`。
- 全局通知通过 `useAppToast()` 工作，四种通知类型与当前主题一致且不会重复堆叠。
- 桌面与移动端布局无溢出或裁切。
- 键盘、焦点和语义状态完整。
- 格式化、类型检查和生产构建通过。
- 没有遗留调试内容、重复实现或废弃代码。
