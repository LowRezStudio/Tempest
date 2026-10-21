// JSON POSTs to the mod repository. Sent from here rather than the renderer because the
// repository drops the body of any request with a foreign Origin (its CSRF guard), and the
// renderer always sends one (http://localhost:1420 in dev, app://- when packaged); requests from
// the main process carry none, like the CLI's.
import { net } from "electron";
import { handle } from "./ipc/handle.js";

handle("repo:post-json", async (_event, { url, body }) => {
	const response = await net.fetch(url, {
		method: "POST",
		headers: { "content-type": "application/json" },
		body: JSON.stringify(body),
	});
	return { status: response.status, body: await response.text() };
});
