package dev.um.wfbox;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.BufferedReader;
import java.io.IOException;
import java.io.InputStreamReader;
import java.io.OutputStream;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.concurrent.ConcurrentLinkedQueue;
import java.util.concurrent.atomic.AtomicReference;
import net.minecraft.server.MinecraftServer;

/**
 * The link to `worldforge serve`.
 *
 * <p>A daemon thread reads JSON lines into {@link #world}, then publishes what
 * arrived by swapping {@link #pending}. The server thread takes the pending flags
 * and reads `world`: that hand-off (a volatile write after the mutation) is what
 * makes it safe to touch the state without a lock. Nothing here touches Minecraft;
 * {@link Canvas} does that, on the server thread.
 */
public final class Bridge {
	/** The world as the last messages described it. Only the link thread writes it. */
	public static final class Snapshot {
		public int width;
		public int height;
		public int baseY = WfBox.DEFAULT_BASE_Y;
		public int seaLevel = 3;
		public int canvasTop = WfBox.DEFAULT_BASE_Y + 44;
		public String tag = "wfbox";
		public String world = "";
		public long seed;
		public String[] palette = new String[0];
		public int generation;
		public JsonArray tiles = new JsonArray();
		public JsonArray structures = new JsonArray();
		public JsonArray units = new JsonArray();
		public JsonArray villages = new JsonArray();
		public JsonArray kingdoms = new JsonArray();
		public JsonArray edits = new JsonArray();
		public int tick;
		public int year;
		public int pop;
		public int villageCount;
		public int kingdomCount;
		public String age = "";
		public String hash = "";
		public List<String> news = new ArrayList<>();

		public String block(int id) {
			return id >= 0 && id < palette.length ? palette[id] : "air";
		}
	}

	/** What arrived since the server last looked. */
	public static final class Pending {
		public volatile boolean fullWorld;
		public volatile boolean edits;
		public volatile boolean structures;
		public volatile boolean frame;
		public volatile boolean notices;
	}

	private static final Snapshot WORLD = new Snapshot();
	private static final AtomicReference<Pending> PENDING = new AtomicReference<>(new Pending());
	private static final ConcurrentLinkedQueue<String> OUTBOX = new ConcurrentLinkedQueue<>();
	/** One-line replies from the bridge, waiting to be shown to the players. */
	private static final List<String> NOTICES = Collections.synchronizedList(new ArrayList<>());
	private static volatile boolean running;
	private static volatile Thread thread;
	private static volatile Socket socket;
	private static volatile String status = "not started";

	public static String status() {
		return status;
	}

	/** The world description. Read it after {@link #takePending()} says what changed. */
	public static Snapshot world() {
		return WORLD;
	}

	/** Claim what arrived; the caller must apply it before taking again. */
	public static Pending takePending() {
		return PENDING.getAndSet(new Pending());
	}

	/** Take every notice that has arrived since the last call. */
	public static List<String> drainNotices() {
		synchronized (NOTICES) {
			List<String> copy = new ArrayList<>(NOTICES);
			NOTICES.clear();
			return copy;
		}
	}

	public static void send(String json) {
		if (running) {
			OUTBOX.add(json);
		}
	}

	/** Ask for the tile field again: after an origin change, or a reconnect. */
	public static void requestWorld() {
		send("{\"t\":\"tiles\"}");
	}

	public static void attach(MinecraftServer server) {
		detach(server);
		WfConfig config = WfConfig.load();
		running = true;
		status = "connecting to 127.0.0.1:" + config.port;

		Thread link = new Thread(() -> loop(config), "wfbox-bridge");
		link.setDaemon(true);
		link.start();
		thread = link;
	}

	public static void detach(MinecraftServer server) {
		running = false;
		Socket open = socket;
		socket = null;
		if (open != null) {
			try {
				open.close();
			} catch (IOException ignored) {
				// closing is best effort
			}
		}
		Thread link = thread;
		thread = null;
		if (link != null) {
			try {
				link.join(500);
			} catch (InterruptedException e) {
				Thread.currentThread().interrupt();
			}
		}
		status = "stopped";
	}

	private static void loop(WfConfig config) {
		while (running) {
			try (Socket link = new Socket()) {
				link.connect(new InetSocketAddress("127.0.0.1", config.port), 1000);
				link.setTcpNoDelay(true);
				socket = link;
				status = "connected to 127.0.0.1:" + config.port;
				WfBox.LOG.info("wfbox: connected to the bridge on 127.0.0.1:{}", config.port);

				BufferedReader in = new BufferedReader(
					new InputStreamReader(link.getInputStream(), StandardCharsets.UTF_8));
				OutputStream out = link.getOutputStream();
				startWriter(link, out);

				// A mod can join mid-simulation: ask for the world description first.
				out.write("{\"t\":\"tiles\"}\n".getBytes(StandardCharsets.UTF_8));
				out.flush();

				String line;
				while (running && (line = in.readLine()) != null) {
					if (!line.isBlank()) {
						read(line);
					}
				}
			} catch (IOException e) {
				status = "no bridge: " + e.getMessage();
			}
			if (running) {
				try {
					Thread.sleep(2000);
				} catch (InterruptedException e) {
					Thread.currentThread().interrupt();
					return;
				}
			}
		}
		status = "stopped";
	}

	private static void startWriter(Socket link, OutputStream out) {
		Thread writer = new Thread(() -> {
			try {
				while (running && !link.isClosed()) {
					String command = OUTBOX.poll();
					if (command == null) {
						Thread.sleep(20);
						continue;
					}
					out.write(command.getBytes(StandardCharsets.UTF_8));
					out.write('\n');
					out.flush();
				}
			} catch (IOException | InterruptedException ignored) {
				// the reader notices the closed socket
			}
		}, "wfbox-bridge-writer");
		writer.setDaemon(true);
		writer.start();
	}

	/** Merge one message into the world and publish what arrived. */
	private static void read(String line) {
		JsonObject message;
		try {
			message = JsonParser.parseString(line).getAsJsonObject();
		} catch (RuntimeException e) {
			WfBox.LOG.warn("wfbox: unreadable message: {}", e.toString());
			return;
		}
		String kind = message.has("t") ? message.get("t").getAsString() : "";
		Snapshot next = new Snapshot();
		boolean fullWorld = false;
		boolean edits = false;
		boolean structures = false;
		boolean frame = false;
		boolean notices = false;

		switch (kind) {
			case "hello" -> {
				next = copyHeader(WORLD);
				if (message.has("size")) {
					next.width = message.getAsJsonArray("size").get(0).getAsInt();
					next.height = message.getAsJsonArray("size").get(1).getAsInt();
				}
				if (message.has("base_y")) {
					next.baseY = message.get("base_y").getAsInt();
				}
				if (message.has("sea_level")) {
					next.seaLevel = message.get("sea_level").getAsInt();
				}
				if (message.has("canvas_top")) {
					next.canvasTop = message.get("canvas_top").getAsInt();
				}
				if (message.has("tag")) {
					next.tag = message.get("tag").getAsString();
				}
				if (message.has("world")) {
					next.world = message.get("world").getAsString();
				}
				if (message.has("seed")) {
					next.seed = message.get("seed").getAsLong();
				}
				if (message.has("palette")) {
					JsonArray palette = message.getAsJsonArray("palette");
					next.palette = new String[palette.size()];
					for (int i = 0; i < palette.size(); i++) {
						next.palette[i] = palette.get(i).getAsString();
					}
				}
			}
			case "tiles" -> {
				next = copyHeader(WORLD);
				JsonArray surface = message.getAsJsonArray("surface");
				JsonArray heights = message.getAsJsonArray("h");
				JsonArray tops = message.getAsJsonArray("top");
				JsonArray water = message.getAsJsonArray("water");
				JsonArray extra = message.getAsJsonArray("extra");
				JsonArray tiles = new JsonArray(surface.size());
				for (int i = 0; i < surface.size(); i++) {
					JsonArray tile = new JsonArray(5);
					tile.add(surface.get(i).getAsInt());
					tile.add(heights.get(i).getAsInt());
					tile.add(tops.get(i).getAsInt());
					tile.add(water.get(i).getAsInt());
					JsonArray extras = extra.get(i).getAsJsonArray();
					JsonArray list = new JsonArray(extras.size());
					for (JsonElement block : extras) {
						list.add(block.getAsInt());
					}
					tile.add(list);
					tiles.add(tile);
				}
				next.tiles = tiles;
				fullWorld = true;
			}
			case "edits" -> {
				next = copyHeader(WORLD);
				JsonArray list = message.getAsJsonArray("tiles");
				JsonArray changed = new JsonArray(list.size() / 2);
				for (int i = 0; i + 1 < list.size(); i += 2) {
					JsonArray pair = new JsonArray(2);
					pair.add(list.get(i).getAsInt());
					pair.add(list.get(i + 1).getAsJsonArray());
					changed.add(pair);
				}
				next.edits = changed;
				edits = true;
			}
			case "structures" -> {
				next = copyHeader(WORLD);
				next.structures = message.getAsJsonArray("ops");
				structures = true;
			}
			case "frame" -> {
				next = copyHeader(WORLD);
				next.tick = intOr(message, "tick", 0);
				next.year = intOr(message, "year", 0);
				next.pop = intOr(message, "pop", 0);
				next.villageCount = intOr(message, "village_count", 0);
				next.kingdomCount = intOr(message, "kingdom_count", 0);
				next.age = message.has("age") ? message.get("age").getAsString() : "";
				next.hash = message.has("hash") ? message.get("hash").getAsString() : "";
				next.units = message.has("units") ? message.getAsJsonArray("units") : new JsonArray();
				next.villages = message.has("villages") ? message.getAsJsonArray("villages") : new JsonArray();
				next.kingdoms = message.has("kingdoms") ? message.getAsJsonArray("kingdoms") : new JsonArray();
				next.news = new ArrayList<>();
				for (JsonElement entry : message.getAsJsonArray("news")) {
					JsonArray pair = entry.getAsJsonArray();
					next.news.add(pair.get(0).getAsString() + ": " + pair.get(1).getAsString());
				}
				frame = true;
			}
			case "pong" -> {
				return;
			}
			case "notice" -> {
				NOTICES.add(message.has("text") ? message.get("text").getAsString() : "?");
				notices = true;
				return;
			}
			default -> {
				return; // unknown kinds are ignored on purpose: the protocol grows
			}
		}

		WORLD.width = next.width;
		WORLD.height = next.height;
		WORLD.baseY = next.baseY;
		WORLD.seaLevel = next.seaLevel;
		WORLD.canvasTop = next.canvasTop;
		WORLD.tag = next.tag;
		WORLD.world = next.world;
		WORLD.seed = next.seed;
		WORLD.palette = next.palette;
		WORLD.tiles = next.tiles;
		WORLD.edits = next.edits;
		WORLD.structures = next.structures;
		WORLD.units = next.units;
		WORLD.villages = next.villages;
		WORLD.kingdoms = next.kingdoms;
		WORLD.tick = next.tick;
		WORLD.year = next.year;
		WORLD.pop = next.pop;
		WORLD.villageCount = next.villageCount;
		WORLD.kingdomCount = next.kingdomCount;
		WORLD.age = next.age;
		WORLD.hash = next.hash;
		WORLD.news = next.news;
		if (fullWorld) {
			WORLD.generation++;
		}

		// Publish: after this swap the server thread is allowed to read WORLD.
		final boolean fw = fullWorld;
		final boolean ed = edits;
		final boolean st = structures;
		final boolean fr = frame;
		final boolean no = notices;
		PENDING.updateAndGet(old -> {
			old.fullWorld |= fw;
			old.edits |= ed;
			old.structures |= st;
			old.frame |= fr;
			old.notices |= no;
			return old;
		});
	}

	private static int intOr(JsonObject message, String key, int fallback) {
		return message.has(key) ? message.get(key).getAsInt() : fallback;
	}

	/** A message that only carries part of the world keeps the rest. */
	private static Snapshot copyHeader(Snapshot from) {
		Snapshot copy = new Snapshot();
		copy.width = from.width;
		copy.height = from.height;
		copy.baseY = from.baseY;
		copy.seaLevel = from.seaLevel;
		copy.canvasTop = from.canvasTop;
		copy.tag = from.tag;
		copy.world = from.world;
		copy.seed = from.seed;
		copy.palette = from.palette;
		copy.generation = from.generation;
		copy.tiles = from.tiles;
		copy.structures = from.structures;
		copy.units = from.units;
		copy.villages = from.villages;
		copy.kingdoms = from.kingdoms;
		copy.edits = from.edits;
		copy.tick = from.tick;
		copy.year = from.year;
		copy.pop = from.pop;
		copy.villageCount = from.villageCount;
		copy.kingdomCount = from.kingdomCount;
		copy.age = from.age;
		copy.hash = from.hash;
		copy.news = from.news;
		return copy;
	}
}
