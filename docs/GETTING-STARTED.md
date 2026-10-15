# Getting started

This guide takes you from nothing to your first imported statement. No
programming knowledge is needed for the installer route. If you want to build
the app yourself, see [Building from source](#building-from-source).

Everything below happens on your own computer. Coin Heaven never needs an
account, and it works with the network off.

## 1. Install

### Using an installer

Open the project's **Releases** page on GitHub, and download:

- **macOS**: the `.dmg` that matches your Mac (Apple silicon or Intel).
- **Windows**: the `.exe` (or `.msi`) installer.

Because the installers are not signed with a paid developer certificate, your
system will warn you the first time:

- **macOS**: if it says the app "can't be opened", open **System Settings →
  Privacy & Security**, scroll to the message about Coin Heaven and click
  **Open Anyway**.
- **Windows**: SmartScreen says "Windows protected your PC". Click **More info**,
  then **Run anyway**.

That warning is about the signature, not about what the app does. If you want to
check the download first, [DISTRIBUTION.md](DISTRIBUTION.md) shows how to verify
its checksum.

### Building from source

You need:

| Tool | macOS | Windows |
|---|---|---|
| Node.js 22+ and pnpm | yes | yes |
| Rust 1.88+ (`rustup`) | yes | yes |
| Compiler tools | Xcode command line tools (`xcode-select --install`) | Visual Studio Build Tools with "Desktop development with C++", and Perl (for example Strawberry Perl) |
| WebView | built in | WebView2 (included in current Windows) |

Then:

```bash
git clone https://github.com/xgarciarosich/coin-heaven.git
cd coin-heaven
pnpm install
pnpm app:build     # produces the installer for your system
# or, to try it without installing:
pnpm app:dev
```

The first build compiles the encryption library from source and takes several
minutes. Later builds are fast.

> The Windows build is newer and less tested than the macOS one. If something
> fails, please open an issue with the error text.

## 2. First launch

The app asks one question before it creates anything: **how should your data be
protected?**

- **A passphrase** you type each time you open the app. This is the stronger
  choice, and the right one on a shared computer.
- **Open automatically.** The key is kept in a file in the app's data folder, so
  the app opens straight away. This is convenient and relies on your computer's
  own login and disk encryption (FileVault on Mac, BitLocker on Windows).

Either way you will then be shown **twelve recovery words**.

> **Write them on paper and keep them somewhere safe.** They are the only way
> back in if you forget the passphrase, lose the key file, or move to a new
> computer. Coin Heaven cannot show them again and there is no reset.

You are asked to confirm a few of them so you know you wrote them down correctly.

## 3. Choose your country and currency

In **Settings → General**, pick a region. This sets the currency, number and
date format, tax defaults and the wording the importer ignores on bank
statements. Two regions ship with the app (United States and Spain). If yours is
missing, [add it](../regions/README.md); it is one small file.

## 4. Bring in your first statement

1. Download a statement from your bank as **CSV, Excel or PDF**.
2. Open **Cash flow → Import** and drop the file in.
3. Coin Heaven shows what it understood: the columns, the dates and a sample of
   transactions. Check that it looks right, choose the account the file belongs
   to and confirm.
4. If you import the same period again, only new transactions are added.

If you would rather look around first, **Settings → General → Try it with sample data**
("Load the sample household") fills the app with a made-up household. You can
remove it again from the same place.

## 5. A short tour

| Where | What it is for |
|---|---|
| **Cash flow → Overview** | Where your money goes by category and month, and your budgets |
| **Cash flow → Monthly plan** | A quick "what is left this month" worksheet |
| **Net worth** | What you own and owe on any date, a portfolio, and its return |
| **Independence** | When work could become optional, and what moves that date |
| **Simulator** | A property plan you write yourself |
| **Assistant** | Questions about your finances (optional, see below) |

## 6. Optional: use an AI model that stays on your machine

The assistant is off by default. To use it without anything leaving your
computer, install [Ollama](https://ollama.com), download a model, and point
Coin Heaven at it in **Settings → AI**. The full steps, including how to let a
local model look up prices on the web, are in [LOCAL-AI.md](LOCAL-AI.md).

## 7. Back up and move

- **Choose a backup folder first.** In **Settings → General → Backups**, pick a
  folder, ideally on another disk or in a cloud-synced folder. Nothing is backed
  up until you do. After that an encrypted copy is written after each import and
  when you close the app, and the oldest copies are removed.
- To move to a new computer: install Coin Heaven there, copy the newest backup
  file (and `atlas.keyinfo` from the same folder) across, and enter your
  passphrase or the twelve words.
- **Settings → General → Your data, as files → Export to a folder…** writes
  your data as plain spreadsheet files. These are *not* encrypted, so keep them
  somewhere safe.

## Where your data lives

| System | Folder |
|---|---|
| macOS | `~/Library/Application Support/com.coinheaven.app` |
| Windows | `%APPDATA%\com.coinheaven.app` |

The database is `atlas.db` in that folder (the file name is a leftover from the
project's first name; it is kept so existing data keeps opening).

## Troubleshooting

| Problem | What to try |
|---|---|
| macOS refuses to open the app | See "Open Anyway" in step 1 |
| Windows SmartScreen blocks it | **More info → Run anyway** |
| "The database is locked" at start | Close any other copy of Coin Heaven and reopen |
| You forgot the passphrase | Use the twelve recovery words on the unlock screen |
| You lost the words and the passphrase | The data cannot be recovered. This is by design. |
| An import looks wrong | Pick the right account and check the date format; you can remove an import from Cash flow → Import |
| The build fails on Windows | Check Perl and the C++ build tools are installed, then open an issue with the error |

Found a bug or have a question? Open an issue on GitHub. Please do not paste
real bank statements or balances into an issue.
