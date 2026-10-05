const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const test = require("node:test");
const vm = require("node:vm");

function loadPropertyInspector() {
	const html = fs.readFileSync(
		path.join(__dirname, "../src/propertyInspector/play.html"),
		"utf8",
	);
	const script = html.match(/<script>([\s\S]*?)<\/script>/)?.[1];
	assert.ok(script, "Property Inspector should contain its script");

	const elements = new Map();
	for (const id of ["effect", "gain", "label", "toggle", "effectName", "clear", "fileInput", "status"]) {
		elements.set(id, {
			value: id === "gain" ? "1" : "",
			checked: false,
			options: [],
			style: {},
			appendChild(option) {
				this.options.push(option);
			},
			click() {},
		});
	}

	class WebSocket {
		static latest;

		constructor() {
			this.sent = [];
			WebSocket.latest = this;
		}

		send(raw) {
			this.sent.push(JSON.parse(raw));
		}
	}

	class FileReader {
		readAsDataURL() {
			this.result = "data:audio/wav;base64,AQID";
			this.onload();
		}
	}

	const context = vm.createContext({
		WebSocket,
		FileReader,
		document: {
			getElementById: (id) => elements.get(id),
			createElement: () => ({}),
		},
		console,
		Promise,
		JSON,
	});
	vm.runInContext(script, context, { filename: "play.html" });
	context.connectElgatoStreamDeckSocket(
		"1234",
		"com.soundbar.play",
		"registerPlugin",
		"{}",
		JSON.stringify({
			context: "action-context",
			payload: { settings: { effect: "", gain: 1, label: "", toggle: false } },
		}),
	);
	WebSocket.latest.sent.length = 0;

	return { context, socket: WebSocket.latest };
}

test("sends imported audio to the plugin handler without saving the file payload as settings", async () => {
	const { context, socket } = loadPropertyInspector();
	const fileInput = { files: [{ name: "beep.wav", size: 3 }], value: "" };

	await context.onFileChosen({ target: fileInput });

	assert.deepEqual(socket.sent, [
		{
			event: "sendToPlugin",
			context: "action-context",
			payload: {
				importData: "data:audio/wav;base64,AQID",
				importName: "beep.wav",
			},
		},
	]);
});
