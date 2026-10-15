# Running the AI features entirely on your own machine

Coin Heaven never needs a hosted model. With Ollama for the model and a search
engine for the web, the assistant, auto-categorising and price lookups all run
without anything leaving your computer except the searches themselves — the
name of an asset, never an amount, and no account or key anywhere.

## 1. The model: Ollama

1. Install [Ollama](https://ollama.com/download) and pull a model:

   ```bash
   ollama pull qwen3:8b
   ```

   Anything that follows instructions and writes JSON reliably will do. As a
   guide: 8 GB of memory runs a 4B model, 16 GB an 8B one, 32 GB a 14B one.
   Bigger is better at categorising and at arithmetic; smaller is faster.

   Use a model from the Ollama library rather than a raw GGUF file you
   imported yourself, unless you gave it a chat template. `ollama show
   <model>` should list a template; a model whose only capability is
   `completion` continues text instead of answering it, and will ramble or
   return nothing whatever it is asked.

2. In Coin Heaven, **Settings → AI → Who answers**: choose *Ollama*, type the model
   name exactly as `ollama list` shows it, and save. No key is needed.

Coin Heaven asks Ollama for a 16k-token context window whatever the model's own
default is. Some models default to a window so large that loading them needs
more memory than the machine has, and everything stalls; this avoids that.

## 2. Web search

A price lookup has to search the web, and a local model cannot. Coin Heaven can do
the searching and give the results to your model. In **Settings → AI → Web
search for a local model** choose one:

- **DuckDuckGo** — nothing to install, no account, no key. Coin Heaven asks
  DuckDuckGo's public web page and reads the results, sending the search the
  way a browser's search box does (a plain GET is answered with a bot
  challenge). DuckDuckGo sees the
  words searched (an asset's name) and your IP address. It is a web page, not
  an official service: it can start refusing requests if used heavily, and its
  layout can change. Coin Heaven reports that plainly when it happens.
- **Exa** — a search service built for AI. It needs an account and an API key
  (new accounts get free credit; see exa.ai/pricing), which you paste into the
  same Settings panel; the key is stored in Coin Heaven's encrypted database. It returns
  each page with the passages that match the question. Exa sees
  the words searched, your IP address and which account the key belongs to.
  Check its privacy policy for how long it keeps queries.
- **SearXNG** — a metasearch engine you run yourself, so the searches never
  leave your machine under your name. More dependable, but you have to run it.

If you chose DuckDuckGo or Exa you can stop here. SearXNG, if you want it:

The quickest way to run it is Docker (Docker Desktop or OrbStack on a Mac):

```bash
mkdir -p ~/searxng && cd ~/searxng
docker run --rm -d --name searxng -p 8888:8080 \
  -v "$PWD:/etc/searxng" searxng/searxng
```

That creates `~/searxng/settings.yml`. Coin Heaven reads results as JSON, which
SearXNG leaves switched off by default, so open that file, find the `search:`
section and make `formats` read:

```yaml
search:
  formats:
    - html
    - json
```

Then restart it:

```bash
docker restart searxng
```

Check it answers:

```bash
curl "http://localhost:8888/search?q=bitcoin+price+eur&format=json" | head -c 300
```

You should see JSON starting with `{"query": ...`. A `403 Forbidden` means the
`json` format is not enabled yet.

Finally, in Coin Heaven, **Settings → AI → Your own search engine**: enter
`http://localhost:8888` and save.

## What happens on a price lookup

1. Coin Heaven shows you the search queries it will run — the names and symbols of
   your holdings, nothing about how much you hold.
2. It runs them on the engine you chose and collects the top results.
3. It hands those results to your model with strict instructions: reply with
   one JSON object and nothing else (and, where the provider supports it,
   switch on its JSON mode so it cannot reply with anything else), never calculate or recall a price, and
   quote the exact words the price was read from. Coin Heaven then checks that the
   quoted words really are in one of the results and contain the number; a
   price that fails is discarded and shown as such, and its source becomes the
   result the words were found in, not the address the model typed.
4. You see each price with its source and accept or reject it. A source that
   was not among the search results is flagged.

Both steps are written to the log in **Settings → AI → Everything that has
been sent**.

## Limits worth knowing

- **Past prices are not fetched this way.** Search snippets rarely carry a
  closing price for a given month. Hosted models with their own search
  (Anthropic, OpenAI, Gemini) can look those up; with a local model, type the
  ones you care about under a holding's *Price* tab, or add *Units held*
  once and none are needed.
- **A small model is a slow reader.** The first answer after starting Ollama
  includes loading the model and can take a minute.
- **Search engines rate-limit.** DuckDuckGo gets one request every second and
  a half; if it still returns nothing, wait a few minutes or switch to
  SearXNG, where you can enable other engines in `settings.yml`.
