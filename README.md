Rust で ISSU できるか実験
===

Rust で ISSU できるか実験したメモ。 TCP Echo サーバを実装してクライアントからの接続を受け付けた状態で接続を切らずにそのまま挙動を変えてみる。

チェックアウトしてビルドして実行する。

```shell
$ git clone https://github.com/m-asama/rust-issu-echo
$ cd rust-issu-echo/echo-dylib
$ cargo build
$ cd ../echo-dylib-filter
$ cargo build
$ cd ../echo-server
$ cargo run
```

別のターミナルから TCP/7777 に接続して何か入力すると入力した文字がそのまま返される。

```shell
$ nc localhost 7777
aaa
aaa
bbb
bbb
```

TCP で接続したまま別のターミナルから `SIGUSR2` を送るとフィルタ機能が有効にされた dylib に切り替わる。 `:set filter upper` と入力すると返ってくる文字が大文字に変換されるようになり、 `:set filter lower` で小文字に変換されるようになり、 `:set filter none` で何も変換されなくなる。

```shell
$ ps -ef | grep echo
m-asama  1720379 1617914  0 22:11 pts/2    00:00:00 target/debug/echo-server
$ kill -USR2 1720379
```

```shell
:set filter upper
:SET FILTER UPPER
aaa
AAA
bbb
BBB
```

`SIGUSR1` を送ると元の dylib に戻り `:set filter upper` とかしても効かなくなる。

