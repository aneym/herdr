# Who holds a terminal: attach and takeover

One client controls a terminal at a time. A second client that attaches with `--takeover`
replaces it, and the replaced client is told "terminal attach taken over".

## The rule (Alex, 2026-10-12)

Any client Alex is actively using can attach to any pane and drive it, including a pinned
agent tab. Only a silent takeover by an idle machine is refused.

- **Agent tabs are homed on the server's machine.** A client that arrived over ssh (another
  machine: Book, PC) may not take a pinned agent tab from a client on this machine while
  someone is typing on this machine: a key, text, paste or click in the last 2 minutes
  (`HOME_HOLDER_ACTIVE_WINDOW` in `src/server/headless.rs`).
- **An idle home holder yields.** If nobody typed on this machine's client in that window,
  the other machine takes the tab over. This is the machine Alex is using.
- **The same holds between two other machines.** A remote client may not take a pinned
  agent tab from another remote client someone typed on in that window (an idle Book Shell
  retook the CoS tab from the PC Alex was using, 2026-10-12 17:06Z).
- **This machine can always take a tab back,** from any client.
- **Plain tabs** keep the plain rule: any `--takeover` replaces the holder.

A refused takeover closes the requesting client with a reason that contains "taken over",
so the Mac and PC Shells show the pane as held, not crashed. The server logs every
takeover (`terminal attach takeover`, with `remote`) and every refusal
(`remote takeover of an agent terminal in use refused`) in `herdr-server.log`.

## History

- e85e2ffb (2026-10-12 15:43Z): remote takeovers of home agent tabs refused, to stop the
  Book's Shell retaking the CoS tab while Alex worked on Studio.
- Follow-up the same day: that refusal also blocked the PC Shell Alex was using from the
  Recruiter tab ("herdr shell pc recruiter says disconnected", 16:57Z). The refusal now
  needs a home holder that is actually in use.
