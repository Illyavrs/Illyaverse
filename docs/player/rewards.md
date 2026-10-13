# Weekly rewards and Stardust

Complete weekly play missions to earn Stardust, then spend it on profile titles and avatar frames. These rewards are cosmetic: they do not change scores, pp or rankings.

## Weekly missions

The week starts on Monday at 00:00 in **Asia/Taipei (UTC+8)**. There are three missions, each worth **20 Stardust**:

| Mission | Requirement |
| --- | --- |
| Keep playing | Submit 5 verified, passed scores |
| Explore beatmaps | Submit verified, passed scores on 3 different beatmaps |
| Try another category | Submit verified, passed scores in 2 different mode/category combinations |

For the third mission, a combination means a game mode together with Normal or Relax. For example, osu! Normal and osu! Relax count as two combinations; so do osu! Normal and taiko Normal. You do not need to play every mode.

Only scores submitted to the server during the current week count. Pending, rejected and failed scores do not count. A manually resubmitted replay must also have been played during the current week and approved by a moderator. Uploading an older play now, or having an older submission approved now, does not make it eligible.

Once you meet a mission's requirement, open the Rewards page and choose **Claim** to receive its 20 Stardust. Claim completed missions during their week. Each mission can be claimed once per week, for a maximum of **60 Stardust**. Retrying a completed claim never pays twice or claims a later week's mission.

There is no daily sign-in requirement or login streak. Mission progress starts over each Monday at 00:00 Taipei time; earned Stardust stays in your wallet.

## The reward shop

The Rewards page shows your missions, wallet, the shop and the items you own. The shop starts empty until an administrator publishes rewards.

- **Titles** add a short label to your website profile and player cards.
- **Avatar frames** surround avatars with artwork uploaded by an administrator. The same frame appears on profiles, player cards, rankings, scores, announcements and chat. Existing line, double and corner borders are also supported.

Choose a published item to buy it at its listed Stardust price, then equip it from your inventory. Your equipped title and frame are visible on your profile and on cards in the online players, friends and groups pages. Your wallet balance and inventory are private and can only be read by you.

Retrying a completed purchase does not charge you again, even if the item's price or availability has since changed.

Administrators can take items off sale. An item you already own stays in your inventory and can still be equipped after it is taken off sale.

## Player previews

On desktop, pause the pointer over a player's linked avatar or name to fade in their player card beside the pointer. You can move into the card to open their profile or use its available social actions. Escape, scrolling or moving away closes the preview. On a touch screen, tapping the link opens the profile directly.

## Creating frame artwork

Administrators with `rewards.manage` can create an unpublished avatar-frame reward, upload its artwork, then publish it in the shop. The management page shows a preview before publication.

For an existing geometric frame, choose the image style and upload artwork directly. The upload replaces its appearance in one operation while retaining its publication status and existing owners.

- Use a **static transparent PNG or WebP**, no larger than **2 MiB**.
- Images must be **square, 128–1024 pixels** on each side. **512 × 512** is recommended.
- The avatar occupies the **central 75%** of the image. For a 512 × 512 image, keep the central **384 × 384** area transparent and draw the frame around it. This leaves a 64-pixel outer band on each side.
- The artwork scales around the avatar without changing its layout size. Uploads retain transparency and are stored as immutable PNG files.

An image frame must have uploaded artwork before it can be published. Replacing artwork updates the reward's revision; a stale edit must be reloaded before retrying. Taking a frame off sale does not remove it from existing owners.
