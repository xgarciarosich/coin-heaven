# Moving to a new computer, and what to do if one is stolen

Your finances live in one encrypted file on your computer. That is good for
privacy, and it means *you* are the backup plan. This guide covers three
moments: moving to a new device on purpose, losing a device, and getting ready
before either happens.

## What you need, in one picture

To open your data on any computer you need **two things**:

| You need | Where it is | Why |
|---|---|---|
| **A copy of the data** | A backup file (`atlas-….db.enc`) in your backup folder | The data itself, encrypted |
| **A way to unlock it** | Your **passphrase** *or* your **twelve recovery words** | The secret that opens it |

Along with the backup, the app saves a small **key-info file** (`atlas-….keyinfo`,
and the latest as `atlas.keyinfo`). It is not secret, but the recovery words
cannot work without it, so **keep it next to the backup**. Coin Heaven puts it
there for you.

If you have the backup, its key-info file, and either secret, you can always get
back in. If you have lost both the passphrase and the words, nobody can open the
data, including us. There is no reset.

## Before anything goes wrong (10 minutes)

1. **Write the twelve words on paper** and keep them somewhere that is not the
   computer, and ideally not the same bag. A safe, a drawer at a relative's.
   Photos of the words and password-manager notes are fine if you trust them
   more than a drawer, but not a file on the same laptop.
2. **Choose a backup folder.** Settings → General → Backups → *Choose a folder*.
   Pick a folder on **another disk or in a cloud drive** (iCloud Drive, Dropbox,
   OneDrive, Syncthing). The copies are encrypted, so the provider cannot read
   them. Until you choose, nothing is backed up.
3. **Press "Back up now"** once, then open the folder and check you can see an
   `atlas-….db.enc` file and an `atlas-….keyinfo` file.
4. **Turn on disk encryption**: FileVault on a Mac, BitLocker on Windows. This
   is what protects the app if you chose "open automatically".
5. **Prefer a passphrase** over "open automatically" on a laptop you carry. With
   a passphrase, no key is stored on the machine at all. You choose this at
   first launch; changing it later is not in the app yet (it is on the
   [roadmap](https://github.com/xgarciarosich/coin-heaven/wiki/Roadmap)).
6. **Try a restore once** (below) on a spare folder or computer, so the first
   time is not the day you need it.

## Moving to a new computer

You are in no hurry here. The old computer still works, so you can do this
calmly.

1. **On the old computer**, open Coin Heaven and press **Back up now** (Settings
   → General → Backups). Wait for "Backup written".
2. **Copy the newest backup to the new computer.** From your backup folder take
   the newest `atlas-<date>.db.enc` **and** its matching
   `atlas-<date>.keyinfo` (the two share the same date and time).
3. **Install Coin Heaven on the new computer** (see
   [GETTING-STARTED.md](GETTING-STARTED.md)). **Do not open it yet.** The first
   launch would offer to set up a brand-new, empty database.
4. **Put the files where the app looks for them.** Create the folder if it does
   not exist:

   | System | Folder |
   |---|---|
   | macOS | `~/Library/Application Support/com.coinheaven.app` |
   | Windows | `%APPDATA%\com.coinheaven.app` |

   Copy the backup in **as `atlas.db`** (rename it) and the key-info file in
   **as `atlas.keyinfo`**:

   ```text
   atlas-20261016T090000Z.db.enc      →   atlas.db
   atlas-20261016T090000Z.keyinfo     →   atlas.keyinfo
   ```
5. **Open Coin Heaven.** It sees an existing database and asks you to unlock it.
   Type your **passphrase**, or choose the recovery words and type the **twelve
   words**.
6. **Check your numbers** against what you remember, then **choose a backup
   folder on the new computer** so you are protected again.

If you chose "open automatically" on the old computer, the new one will ask for
your twelve words the first time, and then remember the key by itself.

**It says "wrong phrase" or "cannot read"?** Check that the key-info file is the
one that came with that backup, that the file is named exactly `atlas.keyinfo`,
and that you typed the twelve words in order with no extras.

## If a computer is stolen or lost

Take a breath. What an attacker gets depends on how you set things up.

| Your setup | What a thief can do |
|---|---|
| **Passphrase** + disk encryption | Almost nothing. The data is encrypted and the key is not on the disk. A weak passphrase can be guessed offline, so see the last step below. |
| **"Open automatically"** + disk encryption | Nothing, as long as they cannot log in as you. The disk is locked. |
| **"Open automatically"**, **no** disk encryption | **Assume they can read everything.** The key sits next to the data. |

### First hour

1. **Lock or wipe the device remotely.** Find My (Apple) or Find My Device
   (Windows/Google). Mark it as lost.
2. **Change the password of the account you use to sign in** to that computer
   and to its cloud (Apple ID, Microsoft account, Google), and sign the device
   out of them.
3. **Revoke any API keys** you gave Coin Heaven, if you used a hosted AI model or
   a search service (for example Exa, OpenAI, Anthropic, DeepSeek). They are
   stored inside the encrypted database; if there is any doubt, revoke them at
   the provider and create new ones.
4. **Delete exports.** If you ever used Settings → *Export to a folder*, those
   spreadsheets are **not encrypted**. If they were on the stolen device or in a
   synced folder, treat them as exposed and remove them from the cloud.

### The next day

5. **Restore on a replacement computer** using the steps above. Your backup
   folder (on another disk or in the cloud) is what saves you here.
6. **Watch for phishing.** Your data holds merchants, balances and account
   nicknames, **not** bank logins or card numbers, but someone who has read it
   can write a convincing message ("we noticed a payment to …"). Be suspicious of
   unexpected calls and messages that mention real details.
7. **Tell your bank** if your statements showed anything sensitive, and keep an
   eye on your accounts. Coin Heaven never held your bank password, so there is
   nothing to reset there.

### If you think they could crack it

This applies if your passphrase was short, guessable or reused, or if you had
no disk encryption and chose "open automatically".

- The strongest move is to **start a fresh database** on the new computer with a
  new, long passphrase, and **import your statements again** (you still have the
  files from your bank). The new database has a new key.
- Changing the passphrase on the existing database does **not** change the key
  underneath it, so anyone who copied the old data and cracked the old
  passphrase could still open the old copy. Starting fresh does.

## If you lost the passphrase or the words

- **Lost the passphrase but have the words:** use the twelve words on the
  unlock screen. Once you are in, make a backup straight away.
- **Lost the words but remember the passphrase:** you can still open it. The
  words cannot be shown a second time, so for a long-term safety net the best
  move is to create a fresh database (new passphrase, new words) and re-import
  your statements.
- **Lost both:** the data cannot be recovered. Start fresh and re-import your
  bank statements.

## Quick checklist

- [ ] Twelve words on paper, away from the computer
- [ ] Backup folder chosen, on another disk or a cloud drive
- [ ] One "Back up now" done and the two files seen in the folder
- [ ] Disk encryption on
- [ ] A restore tried once

*Not financial advice. This guide describes how Coin Heaven stores and restores
its data; it is not a substitute for your bank's or device maker's instructions
about a lost device.*
