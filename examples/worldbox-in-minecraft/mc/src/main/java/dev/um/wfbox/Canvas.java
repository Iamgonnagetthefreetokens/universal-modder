package dev.um.wfbox;

import com.google.gson.JsonArray;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.Iterator;
import java.util.List;
import java.util.Map;
import java.util.Optional;
import java.util.Set;
import net.minecraft.ChatFormatting;
import net.minecraft.core.BlockPos;
import net.minecraft.core.registries.BuiltInRegistries;
import net.minecraft.network.chat.Component;
import net.minecraft.resources.Identifier;
import net.minecraft.server.MinecraftServer;
import net.minecraft.server.level.ServerLevel;
import net.minecraft.world.entity.Entity;
import net.minecraft.world.entity.EntitySpawnReason;
import net.minecraft.world.entity.EntityType;
import net.minecraft.world.entity.Mob;
import net.minecraft.world.level.block.Block;
import net.minecraft.world.level.block.Blocks;
import net.minecraft.world.level.block.state.BlockState;

/**
 * Puts the world where the player can walk around in it.
 *
 * <p>The canvas is a rectangle of the overworld starting at {@code (originX, baseY,
 * originZ)}: tile {@code (col, row)} becomes the column at
 * {@code (originX + col, *, originZ + row)}, built up from the ocean floor.
 *
 * <p>Everything is applied by diffing -- a block is only written when it is not
 * already the block it should be -- and a rebuild is spread over as many ticks as
 * it needs, so a big map never stalls the server.
 */
public final class Canvas {
	/** Ceiling on block writes per tick, so a rebuild never stalls the server. */
	private static final int WRITES_PER_TICK = 20_000;
	/** How long a unit may be missing from the frames before its mob is removed. */
	private static final int MISSING_FRAMES = 6;

	private final MinecraftServer server;
	private final ServerLevel level;
	private final Map<String, BlockState> states = new HashMap<>();
	private final Map<Integer, Entity> proxies = new HashMap<>();
	private final Map<Integer, Integer> missing = new HashMap<>();

	private int originX;
	private int originZ;
	private int generation = -1;
	private boolean built;
	private int lastYear = -1;
	private int dashboardTicks;
	private String status = "no world yet";

	/** The tile field being built, a slice at a time. */
	private JsonArray building;
	private int cursor;
	private int builtWidth;

	public Canvas(MinecraftServer server) {
		this.server = server;
		this.level = server.overworld();
	}

	public String status() {
		return status;
	}

	public int originX() {
		return originX;
	}

	public int originZ() {
		return originZ;
	}

	public int mobCount() {
		return proxies.size();
	}

	/** Move the canvas; the world is rebuilt at the new place. */
	public void setOrigin(int x, int z) {
		originX = x;
		originZ = z;
		restart();
		status = "origin " + x + "," + z;
	}

	/** Rebuild from the tile field the bridge will send next. */
	public void restart() {
		if (built) {
			wipe();
		}
		built = false;
		building = null;
		cursor = 0;
		generation = -1;
		Bridge.requestWorld();
	}

	public void clear() {
		wipe();
		despawnAll();
		built = false;
		building = null;
		generation = -1;
		status = "cleared";
	}

	/** Called from the server tick with whatever the bridge published. */
	public void tick() {
		Bridge.Pending pending = Bridge.takePending();
		Bridge.Snapshot world = Bridge.world();
		if (world.width <= 0 || world.height <= 0) {
			return;
		}
		if (world.generation != generation) {
			if (built) {
				wipe();
			}
			generation = world.generation;
			built = false;
			building = null;
			cursor = 0;
		}
		if (pending.fullWorld && world.tiles.size() > 0) {
			building = world.tiles;
			builtWidth = world.width;
			cursor = 0;
		}
		if (building != null) {
			continueBuilding(world);
		}
		if (pending.edits && world.edits.size() > 0) {
			applyEdits(world);
		}
		if (pending.structures && world.structures.size() > 0) {
			applyStructures(world);
		}
		if (pending.notices) {
			for (String notice : Bridge.drainNotices()) {
				run("tellraw @a [{\"text\":\"worldforge \",\"color\":\"gray\"},{\"text\":\""
					+ escape(notice) + "\",\"color\":\"yellow\"}]");
			}
		}
		if (pending.frame) {
			applyUnits(world);
			dashboard(world);
		}
	}

	// ------------------------------------------------------------------ blocks

	private void continueBuilding(Bridge.Snapshot world) {
		int written = 0;
		while (cursor < building.size() && written < WRITES_PER_TICK) {
			int col = cursor % builtWidth;
			int row = cursor / builtWidth;
			written += placeColumn(world, col, row, building.get(cursor).getAsJsonArray());
			cursor++;
		}
		if (cursor >= building.size()) {
			building = null;
			cursor = 0;
			built = true;
			status = "built " + world.width + "x" + world.height + " at " + originX + "," + originZ;
			forceLoad(world);
		}
	}

	private void applyEdits(Bridge.Snapshot world) {
		for (int i = 0; i < world.edits.size(); i++) {
			JsonArray pair = world.edits.get(i).getAsJsonArray();
			int index = pair.get(0).getAsInt();
			int col = index % world.width;
			int row = index / world.width;
			placeColumn(world, col, row, pair.get(1).getAsJsonArray());
		}
	}

	private void applyStructures(Bridge.Snapshot world) {
		for (int i = 0; i < world.structures.size(); i++) {
			JsonArray op = world.structures.get(i).getAsJsonArray();
			int x = originX + op.get(0).getAsInt();
			int y = op.get(1).getAsInt();
			int z = originZ + op.get(2).getAsInt();
			BlockState state = stateFor(world.block(op.get(3).getAsInt()));
			if (state != null) {
				set(x, y, z, state);
			}
		}
	}

	/**
	 * One tile column: `[surface, h, top, water, extras...]`, the same payload the
	 * bridge sends. Only blocks that differ are written.
	 */
	private int placeColumn(Bridge.Snapshot world, int col, int row, JsonArray tile) {
		if (tile.size() < 4) {
			return 0;
		}
		String surface = world.block(tile.get(0).getAsInt());
		int h = tile.get(1).getAsInt();
		int top = tile.get(2).getAsInt();
		boolean water = tile.get(3).getAsInt() != 0;
		BlockState surfaceState = stateFor(surface);
		BlockState filler = fillerFor(surface);
		int x = originX + col;
		int z = originZ + row;
		int written = 0;
		int columnTop = Math.max(h, top);
		int extras = Math.min(4, Math.max(0, tile.size() - 4));
		for (int y = world.baseY; y <= world.canvasTop; y++) {
			BlockState want = Blocks.AIR.defaultBlockState();
			if (surfaceState != null && y <= world.baseY + h) {
				if (y == world.baseY + h) {
					want = surfaceState;
				} else if (y == world.baseY + h - 1) {
					want = filler;
				} else {
					want = Blocks.STONE.defaultBlockState();
				}
			} else if (water && y > world.baseY + h && y <= world.baseY + top) {
				want = Blocks.WATER.defaultBlockState();
			} else {
				int index = y - world.baseY - columnTop - 1;
				if (index >= 0 && index < extras) {
					BlockState extra = stateFor(world.block(tile.get(4 + index).getAsInt()));
					if (extra != null) {
						want = extra;
					}
				}
			}
			if (set(x, y, z, want)) {
				written++;
			}
		}
		return written;
	}

	private BlockState fillerFor(String surface) {
		return switch (surface) {
			case "sand", "red_sand" -> Blocks.SAND.defaultBlockState();
			case "gravel", "stone", "cobblestone", "smooth_stone", "stone_bricks", "basalt" ->
				Blocks.STONE.defaultBlockState();
			default -> Blocks.DIRT.defaultBlockState();
		};
	}

	/** The block state for a bridge palette name, or null when it is not a block. */
	private BlockState stateFor(String name) {
		if (name == null || name.isEmpty() || name.equals("air")) {
			return Blocks.AIR.defaultBlockState();
		}
		if (states.containsKey(name)) {
			return states.get(name);
		}
		Optional<Block> block = BuiltInRegistries.BLOCK.getOptional(Identifier.withDefaultNamespace(name));
		BlockState state = block.map(Block::defaultBlockState).orElse(null);
		// Mobs live in the same palette, so "unknown" is normal: it is not a block.
		states.put(name, state);
		return state;
	}

	/** Write a block only when it is not already that block. */
	private boolean set(int x, int y, int z, BlockState state) {
		BlockPos pos = new BlockPos(x, y, z);
		if (level.getBlockState(pos) == state) {
			return false;
		}
		level.setBlock(pos, state, Block.UPDATE_CLIENTS | Block.UPDATE_KNOWN_SHAPE);
		return true;
	}

	/** Everything of the canvas back to air, but only what is not air already. */
	private void wipe() {
		Bridge.Snapshot world = Bridge.world();
		if (world.width <= 0 || world.height <= 0) {
			return;
		}
		BlockState air = Blocks.AIR.defaultBlockState();
		for (int row = -1; row <= world.height; row++) {
			for (int col = -1; col <= world.width; col++) {
				if (row >= 0 && row < world.height && col >= 0 && col < world.width) {
					continue; // the body is handled below, column by column
				}
				for (int y = world.baseY - 1; y <= world.canvasTop; y++) {
					set(originX + col, y, originZ + row, air);
				}
			}
		}
		for (int row = 0; row < world.height; row++) {
			for (int col = 0; col < world.width; col++) {
				for (int y = world.baseY - 1; y <= world.canvasTop; y++) {
					set(originX + col, y, originZ + row, air);
				}
			}
		}
	}

	private void forceLoad(Bridge.Snapshot world) {
		run("forceload add " + originX + " " + originZ + " "
			+ (originX + world.width - 1) + " " + (originZ + world.height - 1));
	}

	// ------------------------------------------------------------------- mobs

	private void applyUnits(Bridge.Snapshot world) {
		Set<Integer> seen = new HashSet<>();
		int spawned = 0;
		for (int i = 0; i < world.units.size(); i++) {
			JsonArray unit = world.units.get(i).getAsJsonArray();
			if (unit.size() < 10) {
				continue;
			}
			int id = unit.get(0).getAsInt();
			String kind = world.block(unit.get(1).getAsInt());
			int col = unit.get(2).getAsInt();
			int row = unit.get(3).getAsInt();
			double y = unit.get(4).getAsDouble();
			int hp = unit.get(5).getAsInt();
			boolean glow = unit.get(7).getAsInt() != 0;
			String name = unit.get(8).getAsString();
			String color = unit.get(9).getAsString();
			seen.add(id);

			Entity proxy = proxies.get(id);
			if (proxy == null || proxy.isRemoved()) {
				proxy = spawn(kind, originX + col + 0.5, y, originZ + row + 0.5);
				if (proxy == null) {
					continue;
				}
				proxies.put(id, proxy);
				spawned++;
			}
			proxy.snapTo(originX + col + 0.5, y, originZ + row + 0.5, 0.0F, 0.0F);
			proxy.setCustomName(Component.literal(name + " " + hp).withStyle(styleFor(color)));
			proxy.setCustomNameVisible(true);
			proxy.setGlowingTag(glow);
			missing.remove(id);
		}

		// hysteresis: a unit missing for a few frames in a row loses its mob
		for (Iterator<Map.Entry<Integer, Entity>> it = proxies.entrySet().iterator(); it.hasNext();) {
			Map.Entry<Integer, Entity> entry = it.next();
			if (seen.contains(entry.getKey())) {
				continue;
			}
			int gone = missing.merge(entry.getKey(), 1, Integer::sum);
			if (gone > MISSING_FRAMES || entry.getValue().isRemoved()) {
				entry.getValue().discard();
				missing.remove(entry.getKey());
				it.remove();
			}
		}
		status = "tick " + world.tick + ", " + proxies.size() + " units"
			+ (spawned > 0 ? " (+" + spawned + ")" : "");
	}

	private Entity spawn(String kind, double x, double y, double z) {
		Optional<EntityType<?>> type = BuiltInRegistries.ENTITY_TYPE.getOptional(Identifier.withDefaultNamespace(kind));
		if (type.isEmpty()) {
			return null;
		}
		Entity entity = type.get().create(level, EntitySpawnReason.COMMAND);
		if (entity == null) {
			return null;
		}
		if (entity instanceof Mob mob) {
			mob.setNoAi(true);
			mob.setSilent(true);
			mob.setInvulnerable(true);
			mob.setPersistenceRequired();
		}
		entity.setNoGravity(true);
		entity.addTag(tag());
		entity.snapTo(x, y, z, 0.0F, 0.0F);
		if (!level.addFreshEntity(entity)) {
			return null;
		}
		return entity;
	}

	private String tag() {
		String tag = Bridge.world().tag;
		return tag == null || tag.isEmpty() ? "wfbox" : tag;
	}

	public void despawnAll() {
		for (Entity entity : proxies.values()) {
			entity.discard();
		}
		proxies.clear();
		missing.clear();
		// and anything left over from a previous session
		run("kill @e[tag=" + tag() + "]");
	}

	/** The nearest chat colour to the `#rrggbb` the bridge sent. */
	private static ChatFormatting styleFor(String hex) {
		int r = 255;
		int g = 255;
		int b = 255;
		if (hex != null && hex.length() == 7) {
			try {
				r = Integer.parseInt(hex.substring(1, 3), 16);
				g = Integer.parseInt(hex.substring(3, 5), 16);
				b = Integer.parseInt(hex.substring(5, 7), 16);
			} catch (NumberFormatException ignored) {
				// keep white
			}
		}
		ChatFormatting best = ChatFormatting.WHITE;
		double bestDistance = Double.MAX_VALUE;
		for (ChatFormatting format : COLOR_ORDER) {
			Integer rgb = format.getColor();
			if (rgb == null) {
				continue;
			}
			double distance = Math.pow((rgb >> 16 & 0xff) - r, 2)
				+ Math.pow((rgb >> 8 & 0xff) - g, 2)
				+ Math.pow((rgb & 0xff) - b, 2);
			if (distance < bestDistance) {
				bestDistance = distance;
				best = format;
			}
		}
		return best;
	}

	private static final List<ChatFormatting> COLOR_ORDER = new ArrayList<>(List.of(
		ChatFormatting.WHITE, ChatFormatting.ORANGE, ChatFormatting.MAGENTA, ChatFormatting.LIGHT_PURPLE,
		ChatFormatting.YELLOW, ChatFormatting.GREEN, ChatFormatting.DARK_PURPLE, ChatFormatting.GRAY,
		ChatFormatting.DARK_GRAY, ChatFormatting.AQUA, ChatFormatting.BLUE, ChatFormatting.DARK_BLUE,
		ChatFormatting.DARK_GREEN, ChatFormatting.RED, ChatFormatting.DARK_RED, ChatFormatting.DARK_AQUA,
		ChatFormatting.GOLD, ChatFormatting.BLACK));

	// -------------------------------------------------------------- dashboard

	/** Boss bar for the state of the world, chat for the news. */
	private void dashboard(Bridge.Snapshot world) {
		if (dashboardTicks == 0) {
			run("bossbar add wfbox:world {\"text\":\"worldforge\"}");
			run("bossbar set wfbox:world color blue");
			run("bossbar set wfbox:world max 100");
			run("bossbar set wfbox:world players @a");
		}
		dashboardTicks++;
		if (dashboardTicks % 10 == 1 || world.year != lastYear) {
			lastYear = world.year;
			String caption = "Year " + world.year + "  |  " + world.pop + " people  |  "
				+ world.villageCount + " villages  |  " + world.kingdomCount + " kingdoms  |  " + world.age;
			run("bossbar set wfbox:world name {\"text\":\"" + escape(caption) + "\"}");
			int percent = (int) ((world.tick % 20) * 100 / 20);
			run("bossbar set wfbox:world value " + percent);
		}
		for (String item : world.news) {
			run("tellraw @a [{\"text\":\"worldforge \",\"color\":\"gray\"},{\"text\":\""
				+ escape(item) + "\",\"color\":\"white\"}]");
		}
	}

	private static String escape(String text) {
		return text.replace("\\", "\\\\").replace("\"", "\\\"");
	}

	private void run(String command) {
		try {
			var source = server.createCommandSourceStack().withSuppressedOutput();
			server.getCommands().performPrefixedCommand(source, command);
		} catch (RuntimeException e) {
			// unknown command, no players, wrong version: never worth a crash
			WfBox.LOG.debug("wfbox: `{}`: {}", command, e.toString());
		}
	}
}
