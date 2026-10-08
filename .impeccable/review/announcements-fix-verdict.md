## verdict

1. Resolved — Unsaved history-navigation text is retained in a user-and-permission-scoped in-memory provider across announcement-tab and route Back/Forward. The re-read desktop zh-TW capture visibly restores the authored title/body and explains that recovered text is still unsaved; the phone conflict capture retains its text and blocked-write state. Bound source revalidates private authority before revealing recovery, retains the original revision when the server record changed, requires explicit replacement/discard, and clears retained text on logout, permission loss or account change even while the editor is absent. The final-image log records all 18 announcement cases passing, including route/tab history recovery, unchanged pagination/status history, changed/deleted targets, global scope clearing and zero implicit writes. All five original capture paths were reopened once at original detail and remain valid; all 19 source hashes and five PNG hashes match the confirmation packet bound to web image sha256:e05054dcb4ef2b1acf5b2f4d7702b00069fb71739708d3783479b65ebe3f5ed9 and its labelled disposable fixture.

## remaining

Clear. No regression introduced by the correction batch is visible in the supplied recaptures or fix evidence. Ship covers the scored fix, not the whole surface.

disposition: ship
