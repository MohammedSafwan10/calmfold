// Reads the public update feed (the same file the app checks) so the
// download buttons always point at the latest signed build. Without network
// the last saved copy in src/data/release.json is used.
import { writeFile, readFile } from "node:fs/promises";

const FEED = "https://github.com/MohammedSafwan10/calmfold-releases/releases/latest/download/stable.json";
const OUT = new URL("../src/data/release.json", import.meta.url);

try {
  const res = await fetch(FEED, { redirect: "follow", signal: AbortSignal.timeout(15000) });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const feed = await res.json();
  if (!feed.version || !feed.platforms) throw new Error("not a Calmfold feed");
  await writeFile(OUT, JSON.stringify(feed, null, 2) + "\n");
  console.log(`release: ${feed.version}`);
} catch (err) {
  const saved = JSON.parse(await readFile(OUT, "utf8"));
  console.warn(`release: feed unavailable (${err.message}); using saved ${saved.version}`);
}
