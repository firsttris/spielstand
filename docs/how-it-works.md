# How syncing works

Syncing saves is easy until two PCs disagree. Spielstand is built around one rule: **no save is
ever lost**, whatever the order in which PCs go on and off, games run and networks fail.

## Versions

Every backup of a game is a **version**: a compressed package of the save files, identified by its
content, with the PC, the time, the reason (after a session, before a restore, …) and the version it
was based on (its *parent*). Versions are never changed. A game's history is a tree of versions;
its **current state** (*head*) is the newest version every PC should have.

![The versions of a game, from every PC](screenshots/versions.png)

## The safety rules

1. **Back up before writing.** Before the agent writes saves to a disk, it stores what is on that
   disk as a version of its own (*saved before loading*). If the disk holds something no version has
   yet, it is uploaded first; offline it waits in the agent's outbox.
2. **Write atomically.** New files are written next to the old ones, checked against their hashes and
   only then renamed into place. Files the version does not have go to the agent's trash, not away.
   If anything fails, the previous state is put back.
3. **Decide by ancestry, never by clocks.** Each PC remembers which version it last had (its *base*)
   and what its saves looked like then. That gives four cases:

    | On the PC | On the server | What happens |
    |---|---|---|
    | unchanged | unchanged | nothing |
    | unchanged | a newer version | the PC loads it (*fast-forward*) |
    | changed | unchanged | the PC uploads its state, it becomes the current one |
    | changed | a newer version | **conflict**: both are kept, you choose |

4. **The server moves the current state only by compare-and-swap.** An upload says which version it
   is based on; the server makes it the current state only if that is still the current state. Two
   PCs uploading at the same moment cannot overwrite each other: the second becomes a *branch*.
5. **A running game is never touched.** The agent sees running games (also under Proton) and loads
   nothing while one runs; the web UI shows which PC is playing what.

## Conflicts

A conflict happens when a game was played on two PCs without a sync in between, for example on the
Steam Deck in the garden without Wi-Fi. Both states are stored. The web UI shows them side by side
with time, PC and play time; choosing one makes it the current state. The other one stays a version
you can load any time.

![Choosing between two states of a game](screenshots/conflict.png)

On the PC itself, the launch option asks too: a desktop notification offers *use the newest* or
*keep mine*. Without an answer within a minute the PC keeps its own state, which is also safe.

## Games with Steam Cloud

Games whose saves Steam Cloud already syncs are backed up, so they get versions too, but are not
loaded automatically, to not fight with Steam. Loading such a version by hand from the web UI works.
`restore_steam_cloud_games = true` in the agent's configuration changes that.

## Retention

Old versions are thinned out, never the ones that matter:

- the last **7 days**: every version,
- up to **90 days**: one per day,
- older: one per week.

Never removed: versions you marked *keep*, the current state, every PC's base, open branches and the
newest version of every PC. Removed versions stay in the server's trash for 30 days.
