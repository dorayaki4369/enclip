// enclip 常駐アプリの画面。状態は Rust 側（src-tauri/src/engine.rs）が持ち、ここは表示と操作だけを扱う。
const { invoke } = window.__TAURI__.core;
const { listen } = window.__TAURI__.event;

const el = (id) => document.getElementById(id);

async function refresh() {
	const status = await invoke("status");
	const connection = el("connection");
	connection.textContent = status.connected ? "接続済み" : "未接続";
	connection.classList.toggle("online", status.connected);
	el("paused").checked = status.paused;
	el("error").hidden = status.error === null;
	el("error").textContent = status.error ?? "";

	const list = el("history");
	list.replaceChildren(
		...status.history.map((entry, index) => {
			const item = document.createElement("li");
			const meta = document.createElement("span");
			meta.className = "meta";
			const time = new Date(entry.created_ms).toLocaleString();
			meta.textContent = `${entry.from_self ? "この PC から" : "iPhone から"} ・ ${time}`;
			item.append(entry.text, meta);
			item.addEventListener("click", () => invoke("copy_history", { index }));
			return item;
		}),
	);
}

function showPairing(stage) {
	el("pairing").hidden = stage === "closed";
	el("pairing-scan").hidden = stage !== "scan";
	el("pairing-confirm").hidden = stage !== "confirm";
	el("pairing-message").textContent = "";
}

el("paused").addEventListener("change", (event) => invoke("set_paused", { paused: event.target.checked }));

el("add-device").addEventListener("click", async () => {
	try {
		const svg = await invoke("start_invite");
		el("qr").innerHTML = svg;
		showPairing("scan");
	} catch (error) {
		showPairing("scan");
		el("pairing-message").textContent = `開始できませんでした: ${error}`;
	}
});

el("approve").addEventListener("click", async () => {
	try {
		await invoke("approve_invite");
		showPairing("done");
		el("pairing-message").textContent = "端末を追加しました。";
	} catch (error) {
		el("pairing-message").textContent = `承認できませんでした: ${error}`;
	}
});

el("close-pairing").addEventListener("click", async () => {
	await invoke("cancel_invite");
	showPairing("closed");
});

listen("pairing", (event) => {
	if (event.payload.kind === "code") {
		el("pairing-code").textContent = event.payload.code;
		showPairing("confirm");
	} else {
		el("pairing-message").textContent = `ペアリングに失敗しました: ${event.payload.message}`;
	}
});

listen("pairing-closed", () => showPairing("closed"));

listen("status-changed", () => refresh());

refresh();
