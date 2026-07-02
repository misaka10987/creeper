# creeper

![Crates.io Version](https://img.shields.io/crates/v/creeper)

现代的 Minecraft 包管理器。

```shell
creeper init ${path-to-new-game}
cd ${path-to-new-game}
creeper add vanilla@=1.21.1 mekanism@10 "bsl@*" create@6
creeper launch
```

<img width="2408" height="1354" style="height: auto;" alt="screenshot" src="https://github.com/user-attachments/assets/925917d7-a21a-466f-880e-01f77f120578" />
<p align="center">creeper 启动的 Minecraft 1.21.1 + Mekanism 10.7.19 + Create 6.0.10 + BSL 10.1.3</p>
<p align="center">自动安装了 NeoForge 21.1.234, Sodium 0.6.13 和 Iris 1.8.8</p>

## 安装

```shell
cargo install --git https://github.com/misaka10987/creeper
```

或者在 [Releases](https://github.com/misaka10987/creeper/releases) 下载相应构建。

## 为什么有这个项目？

现在已经有了很多 Minecraft 第三方启动器，包括国内的 *HMCL* , *PCL* , *BakaXL* , 国外的 *Prism Launcher* 等。这些启动器编写得很完善，各种功能齐全，看似编写一个新的启动器已经是没有多少价值的重复劳动。然而，我们认为，即使第三方启动器的技术和规范已经很成熟，对模组玩家来说，仍有三个大问题亟须解决：

- <strong>依赖与冲突。</strong>Minecraft 模组和一般的软件一样，会互相依赖。想要安装一个模组，经常需要先安装其他模组。[mcmod.cn](https://www.mcmod.cn/class/842.html) 就记载了 *红石兵工厂* -> *热力膨胀5* -> *热力基本* -> *CoFH 核心* -> *Redstone Flux API* 的依赖链条。显然，手动遍历整个依赖树，一个个下载文件，是很麻烦的。同时，经常有两个或多个模组冲突的情况，必须去掉其中一个，游戏才能正常运行。排查这些问题也很耗费时间。有时候玩家还必须手动重命名模组文件，通过字母排序调整模组的加载顺序，才能游玩。这也造成很多玩家不愿意更新模组（麻烦，「只要能跑就别动」）。

- <strong>可复现性。</strong>如今，你可以在各个在线平台上看到 Minecraft 游戏错误的求助问题。众所周知，如果不能可靠地复现问题，就只有基于经验的猜测来解决。而由于不同玩家安装的模组不同，相同的模组安装的版本不同，设定了不同的配置项等，即使有了日志和崩溃报告等数据，帮助他人解决游戏问题，依然是一件费时费力的工作。模组玩家经常要自己解决问题，甚至学习相关的计算机技术。这客观上提高了模组游玩的门槛。

- <strong>巨大的整合包。</strong>正因为上述的问题，许多玩家选择直接下载和安装整合包，而不是自己为游戏加装模组。尽管这种方案很便捷，但整合包通常占用巨大的存储空间：假设有十个整合包都安装了机械动力模组，在模组的 `.jar` 文件一样的情况下，仍然会在电脑上保存十份（考虑到 Btrfs, APFS 这样的 CoW 文件系统还不普及，而且一般不会自动去重）。而且，如果玩家想在整合包的基础上，自己添加和修改一些模组，则还是会遇到之前的问题。我们认为，Minecraft 模组的趣味在于玩家可以自己定制游戏体验。要依赖于预制好的整合包，才能方便地游玩，是背道而驰的。

## 这是什么？

正如你所看到的，creeper 更适合被定义为「包管理器」而非 Minecraft 启动器。也就是说，creeper 在原理上更接近 `npm` , `cargo` , 或者 `dnf` , `apt` , `pacman` 这样的包管理器，附加了 Minecraft 启动功能，而非其他的第三方启动器。

<strong>我们认为，解决上文中问题所需要的工作，与现代包管理器的工作是完全一致的。</strong>包管理器通过 SAT 求解来自动满足依赖之间的约束关系，生成依赖锁文件来保证不同机器上的版本一致，并通过软链接和引用计数垃圾清理等手段，节省磁盘空间。Minecraft 模组玩家也应当受益于这些技术。

creeper 是一个包管理器命令行工具。就像其他包管理器，它可以创建项目（游戏实例），添加依赖（模组），生成和使用锁文件，并有用户管理，游戏安装，和启动的功能。

[creeper-registry](https://github.com/misaka10987/creeper-registry) 是一个中心化的软件包仓库。所有人——不只是模组作者——都可以在此打包和发布模组（通过 GitHub Pull Request），以供所有 creeper 用户使用。

creeper 的 UI 基于命令行（不要害怕！）。如果你会操作命令行（其实很简单！），使用过其他的包管理器，或者愿意花一点点时间学习的话，creeper 完全可以作为你日常游玩 Minecraft 用的启动器。同时，creeper 基于 MIT 协议开源。这意味着如果你是 Minecraft 启动器作者或者软件开发者，你完全可以将 creeper 嵌入你的作品中，为你的用户提供更好的 Minecraft 模组游玩体验。

## 关于微软账户支持

将无限期地推迟支持使用微软账户登录游戏。这是因为作者 [misaka10987](mailto:misaka10987@outlook.com) 无法获取免费版 Azure 账户。微软设定应用开发者必须在 Azure 上注册应用，并在 OAuth 标准授权流中提交 `client_id` 才能获得授权。由于这一政策，没有 Azure 账户，不可能为启动器实现登录微软账户的功能。

如果你知道解决方法，欢迎联系作者。不胜感激。
