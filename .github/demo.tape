# SPDX-FileCopyrightText: 2026 Sebastien Rousseau <sebastian.rousseau@gmail.com>
# SPDX-License-Identifier: Apache-2.0 OR MIT

Output .github/demo.gif

Set Shell "bash"
Set FontSize 15
Set Width 1200
Set Height 600
Set Padding 20
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms

Hide
Type "export PS1='$ ' && clear"
Enter
Show

Type "cargo run --example parse_and_query -q"
Enter
Sleep 3s

Type "cargo run --example stream_events -q"
Enter
Sleep 3s

Type "cargo run --example build_a_document -q"
Enter
Sleep 4s
