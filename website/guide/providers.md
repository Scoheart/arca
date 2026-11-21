# Provider 模板

以下目录来自 Arca Core 的 Provider 模板，包含官方 API、地区与 Coding Plan、模型平台、网关和本地服务。搜索名称或类别可以缩小范围。

<ProviderReference />

模板提供默认地址、协议与官方入口，不会自动创建账号、获取 Key 或验证套餐。按自己的区域、账号和 Model ID 编辑连接。Coding Plan 与普通按量 Key 不能仅按相同品牌互换。

同一服务商可以创建多个独立 Provider 实例，分别保存账号、Base URL 和凭据引用。连接实例与模型开发商不是同一概念：通过网关使用某开发商模型时，Provider 仍是实际访问渠道。

本地服务可以配置无鉴权连接。需要账号、资源 ID、端口或其他占位符的模板，需先填写完整地址。

## 免费入口和试用连接

- **BlockRun Free** 可直接创建免 Key 连接。模型发现提供当前可用的免费聊天模型。**OVHcloud AI Endpoints** 可将 Key 留空，以使用匿名限额。 [BlockRun 免费入口](https://blockrun.ai/free) · [OVHcloud 限额](https://docs.ovhcloud.com/en/guides/public-cloud/ai-machine-learning/ai-endpoints-getting-started)
- **Infomaniak AI Services** 需要在 Base URL 中填写真实的产品 ID，并使用为该产品授权的 API Token。开通 API 需要绑定信用卡。 [产品 ID](https://developer.infomaniak.com/docs/api/get/1/ai) · [试用条件](https://www.infomaniak.com/en/support/faq/2845/getting-started-guide-ai-services-sovereign-ai-services)
- **Vertex AI Express** 使用 Google Cloud Express 签发的 Key。Google AI Studio 签发的 Key 使用 Google Gemini 模板。 [Express 设置](https://docs.cloud.google.com/gemini-enterprise-agent-platform/models/start/express-mode/overview)

[添加 Provider 与 Model](/guide/models)
