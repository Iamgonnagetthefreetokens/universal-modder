// =============================================================================
//  Input: keyboard, gamepad and touch all collapse into one small state object
//  that the match engine reads every frame.
// =============================================================================
const KEYMAP = {
  KeyW: 'up', ArrowUp: 'up',
  KeyS: 'down', ArrowDown: 'down',
  KeyA: 'left', ArrowLeft: 'left',
  KeyD: 'right', ArrowRight: 'right',
  ShiftLeft: 'sprint', ShiftRight: 'sprint',
  Space: 'shoot',
  KeyJ: 'pass', KeyZ: 'pass', Comma: 'pass',
  KeyK: 'through', KeyX: 'through', Period: 'through',
  KeyL: 'tackle', KeyC: 'tackle', Slash: 'tackle',
  KeyQ: 'sw',
  KeyE: 'press',
};

export class Input {
  constructor() {
    this.held = new Set();
    this.edges = { pass: false, through: false, tackle: false, sw: false, press: false };
    this.touch = { mx: 0, my: 0, active: false };
    this.buttons = { sprint: false, shoot: false, pass: false, through: false, tackle: false };
    this.padIndex = null;
    this.onPause = null;
    this.onAnyKey = null;
    this.enabled = true;
    window.addEventListener('keydown', (e) => this.onKey(e, true), { passive: false });
    window.addEventListener('keyup', (e) => this.onKey(e, false), { passive: false });
    window.addEventListener('blur', () => this.held.clear());
    window.addEventListener('gamepadconnected', (e) => {
      this.padIndex = e.gamepad.index;
    });
  }

  onKey(e, down) {
    const a = KEYMAP[e.code];
    if (e.code === 'Escape' || e.code === 'KeyP') {
      if (down && this.onPause) this.onPause();
      e.preventDefault();
      return;
    }
    if (!a) {
      if (down && this.onAnyKey) this.onAnyKey(e.code);
      return;
    }
    e.preventDefault();
    if (down) {
      if (!this.held.has(a) && this.edges[a] !== undefined) this.edges[a] = true;
      this.held.add(a);
    } else {
      this.held.delete(a);
    }
  }

  setStick(mx, my, active) {
    this.touch.mx = mx;
    this.touch.my = my;
    this.touch.active = active;
  }

  setButton(name, down) {
    this.buttons[name] = down;
    if (down && this.edges[name] !== undefined) this.edges[name] = true;
  }

  poll() {
    let mx = 0;
    let my = 0;
    if (this.held.has('left')) mx -= 1;
    if (this.held.has('right')) mx += 1;
    if (this.held.has('up')) my -= 1;
    if (this.held.has('down')) my += 1;
    let sprint = this.held.has('sprint') || this.buttons.sprint;
    let shoot = this.held.has('shoot') || this.buttons.shoot;

    const pads = navigator.getGamepads ? navigator.getGamepads() : [];
    const pad = this.padIndex != null ? pads[this.padIndex] : [].find.call(pads || [], Boolean);
    if (pad) {
      const [lx, ly] = [pad.axes[0] || 0, pad.axes[1] || 0];
      if (Math.hypot(lx, ly) > 0.22) {
        mx = lx;
        my = ly;
      }
      const b = pad.buttons;
      const press = (i) => b[i] && (b[i].pressed || b[i].value > 0.5);
      if (press(7)) shoot = true;
      if (press(6) || press(5)) sprint = true;
      const edgeIf = (i, key) => {
        const on = press(i);
        if (on && !this['_p' + i]) this.edges[key] = true;
        this['_p' + i] = on;
      };
      edgeIf(0, 'pass');
      edgeIf(2, 'through');
      edgeIf(1, 'tackle');
      edgeIf(3, 'sw');
    }
    if (this.touch.active) {
      mx = this.touch.mx;
      my = this.touch.my;
    }
    const mag = Math.hypot(mx, my);
    if (mag > 1) {
      mx /= mag;
      my /= mag;
    }
    const out = {
      mx,
      my,
      sprint,
      shoot,
      pass: this.edges.pass,
      through: this.edges.through,
      tackle: this.edges.tackle,
      sw: this.edges.sw,
      press: this.edges.press,
    };
    this.edges = { pass: false, through: false, tackle: false, sw: false, press: false };
    return out;
  }
}
