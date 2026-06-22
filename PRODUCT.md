# Product

## Register

product

## Users

LitScout-RS 面向需要快速调研 GitHub 项目和 arXiv 论文的学生、研究者和开发者。用户通常在浏览器工作台里完成主题输入、计划确认、证据核对、报告阅读和单篇论文深读。

## Product Purpose

LitScout-RS 把中文调研主题转换为可检查的研究计划，默认抓取 GitHub 和 arXiv 资料，生成带引用的中文报告，并支持把 arXiv 论文加入阅读库做全文笔记和单篇追问。

当前版本默认仍是 GitHub + arXiv 双源研究侦察工作台；用户显式启用扩展学术源后，可以额外接入 Semantic Scholar、DBLP、OpenAlex 和 Crossref。扩展源候选会先经过 EvidenceQualityGate，只有通过主题相关性和可验证元数据筛选的条目会进入报告证据池。后续扩展仍优先考虑 ACL Anthology、OpenReview 等结构化来源，再考虑辅助 Web 证据层；开放网页不会成为默认核心来源。扩源路线详见 `SEARCH_BOUNDARY_EXPANSION_PLAN.md`。

## Brand Personality

克制、清晰、研究导向。界面语气应直接说明当前状态和下一步动作，不写营销式文案。

## Anti-references

不要做成营销落地页，不使用大面积装饰性卡片、复杂动效或会抢走正文注意力的视觉元素。阅读库里论文笔记和追问内容要比统计信息、介绍文案更显眼。

## Design Principles

- 把主要任务放在主视野，辅助信息允许折叠。
- 状态要对应具体对象，避免一个任务影响整页按钮文案。
- 失败信息要说明卡在哪一步，并给出可重试入口。
- 布局服务阅读和判断，不为装饰制造额外层级。

## Accessibility & Inclusion

默认按产品型工作台处理。交互控件需要明确标签、焦点状态和禁用状态；布局在窄屏下应退化为单列；动效只用于状态反馈，并尊重 reduced motion。
