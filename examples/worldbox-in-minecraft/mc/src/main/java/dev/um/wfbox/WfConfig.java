package dev.um.wfbox;

import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;

/**
 * `config/wfbox.json`, written with the defaults on first run:
 *
 * <pre>
 * {
 *   "port": 25607,
 *   "origin_x": 0,
 *   "origin_z": 0
 * }
 * </pre>
 *
 * <p>The origin is where tile (0, 0) lands: the map is built east and south of it,
 * from y = 64 upward, so pick a flat, empty place (a superflat "void" world is
 * the intended canvas).
 */
public final class WfConfig {
	public int port = 25607;
	public int originX;
	public int originZ;

	public static WfConfig load() {
		WfConfig config = new WfConfig();
		Path path = Path.of("config", "wfbox.json");
		if (Files.exists(path)) {
			try {
				String text = Files.readString(path, StandardCharsets.UTF_8);
				JsonObject json = JsonParser.parseString(text).getAsJsonObject();
				if (json.has("port")) {
					config.port = json.get("port").getAsInt();
				}
				if (json.has("origin_x")) {
					config.originX = json.get("origin_x").getAsInt();
				}
				if (json.has("origin_z")) {
					config.originZ = json.get("origin_z").getAsInt();
				}
			} catch (IOException | RuntimeException e) {
				WfBox.LOG.warn("wfbox: config/wfbox.json is unreadable, using defaults: {}", e.toString());
			}
			return config;
		}
		try {
			Files.createDirectories(path.getParent());
			String text = "{\n  \"port\": " + config.port + ",\n  \"origin_x\": " + config.originX
				+ ",\n  \"origin_z\": " + config.originZ + "\n}\n";
			Files.writeString(path, text, StandardCharsets.UTF_8);
		} catch (IOException e) {
			WfBox.LOG.warn("wfbox: could not write {}: {}", path, e.toString());
		}
		return config;
	}
}
