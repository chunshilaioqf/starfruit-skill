# SKILL 命名说明（T-03 定稿）

## 1. 命名要求

- 水果 / 动物风格，读起来轻快好记；
- 贴合功能本质：**星型拓扑的消息枢纽** + **可观测的消息流转**。

## 2. 候选名单

| 候选 | 类型 | 契合点 | 不足 |
|---|---|---|---|
| **starfruit 杨桃** ✅ | 水果 | 杨桃横切面是**五角星**，天然对应**星型（hub-spoke）拓扑**；"star" 直指核心架构 | — |
| pomelo 柚子 | 水果 | 果实大、居中聚合，像中心 hub | 无"星"意象，稍显笨重 |
| mangosteen 山竹 | 水果 | 瓣瓣分明，像独立节点 | 与拓扑/流转无关联 |
| kiwi 猕猴桃 | 水果+动物双关 | 双关有趣 | 语义太泛，指向不明 |
| octopus 章鱼 | 动物 | 中央脑 + 多爪路由很贴切 | 略负面联想；风格偏"怪" |
| longan 龙眼 | 水果 | "眼"呼应观测面板 | 与拓扑无关 |
| rambutan 红毛丹 | 水果 | 毛刺放射像连线 | 拼写不友好 |

## 3. 定稿：**starfruit（杨桃）**

**理由**：杨桃横切面的五角星 = 星型拓扑（唯一 hub 居中、节点辐射连接），
名字本身就是架构图；水果风格、英文短小、拼写友好、无歧义。

## 4. 名称矩阵（全项目统一使用）

| 对象 | 名称 |
|---|---|
| SKILL / 协议 | `starfruit` / starfruit protocol v2 |
| 主节点二进制（Rust crate） | `starfruit-master` |
| 子节点 Rust | `starfruit-node` |
| 子节点 Python | `starfruit_node.py` |
| 子节点 Node.js | `starfruit-node.js` |
| Web 面板 | starfruit panel |
| 运行数据目录 | 默认用户级 `~/.starfruit`（主节点 `master/`、子节点 `nodes/<name>/`）；`--data-dir` 可覆盖（日志、pidfile、wake.log、历史 JSONL） |
| 节点名 | 全网唯一（创建与登录双重校验；在线重名回执 `name_conflict`，客户端退出码 4） |
