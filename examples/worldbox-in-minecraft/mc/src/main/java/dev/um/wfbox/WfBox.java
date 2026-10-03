package dev.um.wfbox;

import net.fabricmc.api.ModInitializer;
import net.fabricmc.fabric.api.command.v2.CommandRegistrationCallback;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerLifecycleEvents;
import net.fabricmc.fabric.api.event.lifecycle.v1.ServerTickEvents;
import net.minecraft.server.MinecraftServer;
import org.slf4j.Logger;
import org.slf4j.LoggerFactory;

/**
 * worldforge in Minecraft.
 *
 * <p>The simulation runs in Rust (`worldforge serve`) and this mod is the thin
 * end: it connects to the bridge on 127.0.0.1, builds the block world it is told
 * about, moves the mobs that stand for units, and shows the state of the world in
 * a boss bar.
 *
 * <p>Everything that decides <em>how</em> the world looks -- block palette,
 * column heights, kingdom borders, building shapes, which mob is which -- is
 * computed in Rust (see {@code worldforge/src/bridge.rs}). This side only applies
 * it, which is why it stays small enough to read in one sitting.
 */
public class WfBox implements ModInitializer {
	public static final String ID = "wfbox";
	public static final Logger LOG = LoggerFactory.getLogger(ID);

	/** The canvas for the world that is loaded, or null when none is. */
	public static volatile Canvas canvas;

	@Override
	public void onInitialize() {
		ServerLifecycleEvents.SERVER_STARTED.register(Bridge::attach);
		ServerLifecycleEvents.SERVER_STOPPING.register(server -> {
			if (canvas != null) {
				canvas.despawnAll();
				canvas = null;
			}
			Bridge.detach(server);
		});
		ServerTickEvents.END_SERVER_TICK.register(WfBox::tick);
		CommandRegistrationCallback.EVENT.register((dispatcher, access, environment) -> WfCommands.register(dispatcher));
		LOG.info("wfbox loaded: run `/wf help` after joining a world");
	}

	private static void tick(MinecraftServer server) {
		if (canvas == null) {
			Canvas fresh = new Canvas(server);
			WfConfig config = WfConfig.load();
			fresh.setOrigin(config.originX, config.originZ);
			canvas = fresh;
			LOG.info("wfbox: canvas starts at {},{} (the bridge will say how tall)", config.originX, config.originZ);
		}
		canvas.tick();
	}
}
