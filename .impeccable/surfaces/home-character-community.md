# Interactive homepage character and community uploads

This is a narrow refinement of the incumbent Persuade landing. The user wants
the supplied character to respond to hovering, and the latest-map section to
show only recent player uploads to this server. Keep the filled wordmark,
original PNG, portrait scale/crop, dense rows, neutral adjustable glass,
ordinary multilingual copy and all real destinations.

THESIS: The original character acknowledges a nearby pointer with a small
face-centered lean, while the records below identify real community uploads.

FOCAL MOMENT: A fine hovering pointer moves the portrait at most 4px sideways,
2px vertically and 0.9 degrees. The pivot remains near its face; the existing
crop and surrounding layout stay fixed. On leaving, window blur, document
hide or preference changes, it eases back to rest. No idle loop, added art,
click action, sound or focus stop is introduced. The accepted single wordmark
fill remains unchanged.

CONTINUITY AND FEEDBACK: Hovering produces a direct visual acknowledgement.
The portrait remains decorative; primary actions preserve normal keyboard
access. Coarse/touch input and reduced motion keep the original static
composition, including an immediate return after a live preference change.

BUDGET: Pointer writes are coalesced through requestAnimationFrame and only
transform the supplied image. No animation schedules work when idle; no new
dependency, image filters, animated glass or background effects are needed.

RECORD TRUTH: `/beatmapsets?source=local&uploaded_only=true&sort=recent&page_size=3`
selects active uploaded local difficulties with no official identity before
grouping by native set. The endpoint orders sets by their most recent upload
timestamp. Home displays one dense
representative per returned local set, with a defensive check of its source,
null official IDs, valid uploader and canonical local namespace. Legacy official sets bearing
a local source label are also excluded. Imported official maps never populate
an empty community list.
View all opens the same local filter in the server catalogue.

FINISH: Root owns the shared browser/build slot. Inspect desktop and phone
with both themes, one pointer response and return, reduced motion, touch,
populated local sets, a true empty list, load/error retry and official-only
unexpected payload. Run the existing portrait geometry/dense-card regression
matrix in that batched pass. Preserve historical captures; allow at most one
correction and confirmation round. No native-client claim follows from these
synthetic website checks.
