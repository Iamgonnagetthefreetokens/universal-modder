package dev.um.wfbox;

import com.mojang.brigadier.CommandDispatcher;
import com.mojang.brigadier.arguments.DoubleArgumentType;
import com.mojang.brigadier.arguments.IntegerArgumentType;
import com.mojang.brigadier.arguments.StringArgumentType;
import net.minecraft.commands.CommandSourceStack;
import net.minecraft.commands.Commands;
import net.minecraft.network.chat.Component;
import net.minecraft.server.level.ServerPlayer;

/** `/wf ...`: the player's side of the bridge. */
public final class WfCommands {
	private WfCommands() {
	}

	public static void register(CommandDispatcher<CommandSourceStack> dispatcher) {
		dispatcher.register(Commands.literal("wf")
			.requires(source -> true)
			.executes(context -> help(context.getSource()))
			.then(Commands.literal("help").executes(context -> help(context.getSource())))
			.then(Commands.literal("info").executes(context -> info(context.getSource())))
			.then(Commands.literal("origin").executes(context -> {
				Canvas canvas = WfBox.canvas;
				ServerPlayer player = context.getSource().getPlayerOrException();
				if (canvas == null) {
					return note(context.getSource(), "no canvas yet");
				}
				canvas.setOrigin((int) Math.floor(player.getX()), (int) Math.floor(player.getZ()));
				canvas.clear();
				canvas.restart();
				return note(context.getSource(), "canvas now starts where you stand");
			}))
			.then(Commands.literal("clear").executes(context -> {
				Canvas canvas = WfBox.canvas;
				if (canvas == null) {
					return note(context.getSource(), "no canvas yet");
				}
				canvas.clear();
				return note(context.getSource(), "canvas cleared");
			}))
			.then(Commands.literal("pause").executes(context -> {
				Bridge.send("{\"t\":\"pause\",\"on\":true}");
				return note(context.getSource(), "worldforge paused");
			}))
			.then(Commands.literal("resume").executes(context -> {
				Bridge.send("{\"t\":\"pause\",\"on\":false}");
				return note(context.getSource(), "worldforge running");
			}))
			.then(Commands.literal("step")
				.then(Commands.argument("ticks", IntegerArgumentType.integer(1, 100000)).executes(context -> {
					int ticks = IntegerArgumentType.getInteger(context, "ticks");
					Bridge.send("{\"t\":\"step\",\"ticks\":" + ticks + "}");
					return note(context.getSource(), "stepping " + ticks + " ticks");
				})))
			.then(Commands.literal("speed")
				.then(Commands.argument("tps", DoubleArgumentType.doubleArg(0.1, 60.0)).executes(context -> {
					double tps = DoubleArgumentType.getDouble(context, "tps");
					Bridge.send("{\"t\":\"speed\",\"tps\":" + tps + "}");
					return note(context.getSource(), "speed " + tps + " ticks/s");
				})))
			.then(Commands.literal("power")
				.then(Commands.argument("name", StringArgumentType.word()).executes(context -> {
					Canvas canvas = WfBox.canvas;
					if (canvas == null) {
						return note(context.getSource(), "no canvas yet");
					}
					String name = StringArgumentType.getString(context, "name");
					ServerPlayer player = context.getSource().getPlayerOrException();
					int col = (int) Math.floor(player.getX()) - canvas.originX();
					int row = (int) Math.floor(player.getZ()) - canvas.originZ();
					if (col < 0 || row < 0) {
						return note(context.getSource(), "you are outside the canvas");
					}
					Bridge.send("{\"t\":\"power\",\"name\":\"" + name + "\",\"col\":" + col + ",\"row\":" + row + "}");
					return note(context.getSource(), "asked for " + name + " at tile " + col + "," + row);
				})))
			.then(Commands.literal("spawn")
				.then(Commands.argument("race", StringArgumentType.word()).executes(context -> {
					Canvas canvas = WfBox.canvas;
					if (canvas == null) {
						return note(context.getSource(), "no canvas yet");
					}
					String race = StringArgumentType.getString(context, "race");
					ServerPlayer player = context.getSource().getPlayerOrException();
					int col = (int) Math.floor(player.getX()) - canvas.originX();
					int row = (int) Math.floor(player.getZ()) - canvas.originZ();
					Bridge.send("{\"t\":\"spawn\",\"race\":\"" + race + "\",\"col\":" + col + ",\"row\":" + row + "}");
					return note(context.getSource(), "asked for a " + race + " at tile " + col + "," + row);
				})))
			.then(Commands.literal("status").executes(context -> {
				Canvas canvas = WfBox.canvas;
				String where = canvas == null ? "no canvas" : canvas.status() + " (" + canvas.originX() + ","
					+ canvas.originZ() + ", " + canvas.mobCount() + " mobs)";
				return note(context.getSource(), Bridge.status() + " | " + where);
			})));
	}

	private static int help(CommandSourceStack source) {
		source.sendSuccess(() -> Component.literal("""
			wfbox - a worldforge world, rendered in Minecraft
			  /wf status            what the bridge and the canvas are doing
			  /wf info              the state of the simulated world
			  /wf pause | resume    stop and start the simulation
			  /wf step <ticks>      advance it by hand
			  /wf speed <tps>       ticks per second
			  /wf power <name>      cast a power at the tile you stand on
			  /wf spawn <race>      drop a creature there
			  /wf origin            move the canvas to where you stand
			  /wf clear             wipe the canvas and the mobs"""), false);
		return 1;
	}

	private static int info(CommandSourceStack source) {
		Bridge.Snapshot world = Bridge.world();
		return note(source, "year " + world.year + " tick " + world.tick
			+ " | pop " + world.pop
			+ " | villages " + world.villageCount
			+ " | kingdoms " + world.kingdomCount
			+ " | " + world.age + " | " + world.hash);
	}

	private static int note(CommandSourceStack source, String text) {
		source.sendSuccess(() -> Component.literal("wfbox: " + text), false);
		return 1;
	}
}
