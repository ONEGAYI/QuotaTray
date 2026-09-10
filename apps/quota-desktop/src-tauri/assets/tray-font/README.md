# 托盘数字字体

`QuotaTrayTrayDigits-Bold.ttf` 是 Noto Sans Bold 的子集，只含 `0123456789kMB.`。
用于原生托盘的细环大字，不依赖用户安装的字体，也不向前端传输任何数据。

- 来源：[Noto Sans Bold](https://github.com/notofonts/noto-fonts/blob/main/hinted/ttf/NotoSans/NotoSans-Bold.ttf)（上游归档仓库）。
- 原文件 SHA-256：`c976e4b1b99edc88775377fcc21692ca4bfa46b6d6ca6522bfda505b28ff9d6a`。
- 许可证：SIL OFL 1.1，全文见同目录 `OFL.txt`；字体内 name ID 13 也保留全文，随二进制嵌入分发。
- 制作：FontTools 4.64.0 的 `subset`，字符集如上，`name_IDs=['*']`、`name_languages=['*']`、`name_legacy=True`、`layout_features=[]`。
- 修改：将 name ID 1/3/4/16 改为 `QuotaTray Tray Digits Bold`，ID 6 改为 `QuotaTrayTrayDigits-Bold`；在 Windows Unicode 英语 name ID 13 写入 `OFL.txt` 全文。

运行时由 `ttf-parser` 读取轮廓，再由 `tiny-skia` 直接栅格化到目标像素画布。
