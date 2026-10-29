import {
  Button,
  H1,
  Row,
  Stack,
  Text,
  useHostTheme,
  useRef,
  useState,
} from "cursor/canvas";

const VIEWBOX = { w: 640, h: 480 };
const PHOTO = { x: 48, y: 36, w: 544, h: 408 };
const CROP = { x: 108, y: 84, w: 424, h: 300 };

type PointEvent = {
  clientX: number;
  clientY: number;
  pointerId: number;
  currentTarget: HTMLDivElement;
};

type ViewId =
  | "current"
  | "original"
  | "stored"
  | "original-frame"
  | "stored-frame";

const VIEWS: { id: ViewId; label: string; needsSave: boolean }[] = [
  { id: "original", label: "O", needsSave: false },
  { id: "original-frame", label: "Of", needsSave: false },
  { id: "stored", label: "S", needsSave: true },
  { id: "stored-frame", label: "Sf", needsSave: true },
];

type Sliders = {
  exposure: number;
  blackpoint: number;
  shadow: number;
  highlight: number;
  brightness: number;
  contrast: number;
  vibrance: number;
  saturation: number;
  clarity: number;
  quality: number;
};

const INITIAL: Sliders = {
  exposure: 0.3,
  blackpoint: 8,
  shadow: 20,
  highlight: -15,
  brightness: 0,
  contrast: 10,
  vibrance: 12,
  saturation: 0,
  clarity: 5,
  quality: 90,
};

export default function EditModeWireframe() {
  const theme = useHostTheme();
  const svgRef = useRef<SVGSVGElement | null>(null);
  const [panelWidth, setPanelWidth] = useState(300);
  const [guide, setGuide] = useState<"none" | "thirds" | "center" | "pointer">(
    "none",
  );
  const [view, setView] = useState<ViewId>("current");
  const [hasSave, setHasSave] = useState(true);
  const [rotation, setRotation] = useState(-2.4);
  const [savedRotation, setSavedRotation] = useState(-2.4);
  const [sliders, setSliders] = useState<Sliders>(INITIAL);
  const [pointer, setPointer] = useState<{ x: number; y: number } | null>(null);

  const usesCurrentGeometry = view !== "original" && view !== "stored";
  const frame = usesCurrentGeometry ? CROP : PHOTO;
  const shownRotation =
    view === "original" ? 0 : view === "stored" ? savedRotation : rotation;
  const photoCenter = {
    x: PHOTO.x + PHOTO.w / 2,
    y: PHOTO.y + PHOTO.h / 2,
  };
  const photoTransform = `rotate(${-shownRotation} ${photoCenter.x} ${photoCenter.y})`;

  function toViewBox(clientX: number, clientY: number) {
    const svg = svgRef.current;
    if (!svg) return null;
    const rect = svg.getBoundingClientRect();
    if (rect.width === 0 || rect.height === 0) return null;
    return {
      x: ((clientX - rect.left) / rect.width) * VIEWBOX.w,
      y: ((clientY - rect.top) / rect.height) * VIEWBOX.h,
    };
  }

  function editCurrent() {
    if (view !== "current") setView("current");
  }

  function setSlider(key: keyof Sliders, value: number) {
    setSliders((prev) => ({ ...prev, [key]: value }));
    editCurrent();
  }

  const sky = theme.chart.sequence[0];
  const ground = theme.chart.sequence[1];
  const building = theme.chart.sequence[2];

  return (
    <Stack gap={16}>
      <Stack gap={4}>
        <H1>Edit mode</H1>
        <Text tone="secondary">
          Wireframe of the same window. The picture starts at about three
          quarters of the width. The divider drags. Rotation is a slider.
          The frame is dragged on the picture.
        </Text>
      </Stack>

      <div
        style={{
          display: "flex",
          height: 680,
          border: `1px solid ${theme.stroke.primary}`,
          background: theme.bg.chrome,
          minWidth: 0,
        }}
      >
        <Stack gap={0} style={{ flex: 1, minWidth: 0 }}>
          <Row
            align="center"
            justify="space-between"
            style={{
              padding: "6px 10px",
              borderBottom: `1px solid ${theme.stroke.tertiary}`,
            }}
          >
            <Text size="small" weight="semibold">
              myimage.jpg
            </Text>
            <Text size="small" tone="secondary">
              {view === "current"
                ? "Current"
                : VIEWS.find((item) => item.id === view)?.label}
            </Text>
          </Row>
          <div style={{ flex: 1, minHeight: 0, background: theme.bg.editor }}>
            <svg
              ref={svgRef}
              viewBox={`0 0 ${VIEWBOX.w} ${VIEWBOX.h}`}
              width="100%"
              height="100%"
              preserveAspectRatio="xMidYMid meet"
              onPointerMove={(event: PointEvent) => {
                const point = toViewBox(event.clientX, event.clientY);
                if (!point) return;
                const inside =
                  point.x >= PHOTO.x &&
                  point.x <= PHOTO.x + PHOTO.w &&
                  point.y >= PHOTO.y &&
                  point.y <= PHOTO.y + PHOTO.h;
                setPointer(inside ? point : null);
              }}
              onPointerLeave={() => setPointer(null)}
            >
              <defs>
                <mask id="frame-hole">
                  <rect
                    x={0}
                    y={0}
                    width={VIEWBOX.w}
                    height={VIEWBOX.h}
                    fill="white"
                  />
                  <rect
                    x={frame.x}
                    y={frame.y}
                    width={frame.w}
                    height={frame.h}
                    fill="black"
                  />
                </mask>
              </defs>
              <g transform={photoTransform}>
                <rect
                  x={PHOTO.x}
                  y={PHOTO.y}
                  width={PHOTO.w}
                  height={PHOTO.h * 0.46}
                  fill={sky}
                />
                <rect
                  x={PHOTO.x}
                  y={PHOTO.y + PHOTO.h * 0.46}
                  width={PHOTO.w}
                  height={PHOTO.h * 0.54}
                  fill={ground}
                />
                <rect
                  x={PHOTO.x + 300}
                  y={PHOTO.y + 150}
                  width={70}
                  height={120}
                  fill={building}
                />
                <rect
                  x={PHOTO.x + 330}
                  y={PHOTO.y + 118}
                  width={14}
                  height={36}
                  fill={building}
                />
              </g>

              {usesCurrentGeometry && (
                <g transform={photoTransform} mask="url(#frame-hole)">
                  <rect
                    x={PHOTO.x}
                    y={PHOTO.y}
                    width={PHOTO.w}
                    height={PHOTO.h}
                    fill={theme.fill.primary}
                  />
                </g>
              )}

              {guide === "thirds" && (
                <g stroke={theme.text.primary} strokeWidth={1} fill="none">
                  <line
                    x1={frame.x + frame.w / 3}
                    y1={frame.y}
                    x2={frame.x + frame.w / 3}
                    y2={frame.y + frame.h}
                  />
                  <line
                    x1={frame.x + (2 * frame.w) / 3}
                    y1={frame.y}
                    x2={frame.x + (2 * frame.w) / 3}
                    y2={frame.y + frame.h}
                  />
                  <line
                    x1={frame.x}
                    y1={frame.y + frame.h / 3}
                    x2={frame.x + frame.w}
                    y2={frame.y + frame.h / 3}
                  />
                  <line
                    x1={frame.x}
                    y1={frame.y + (2 * frame.h) / 3}
                    x2={frame.x + frame.w}
                    y2={frame.y + (2 * frame.h) / 3}
                  />
                </g>
              )}

              {guide === "center" && (
                <g stroke={theme.text.secondary} strokeWidth={1} fill="none">
                  <line
                    x1={frame.x + frame.w / 2}
                    y1={frame.y}
                    x2={frame.x + frame.w / 2}
                    y2={frame.y + frame.h}
                  />
                  <line
                    x1={frame.x}
                    y1={frame.y + frame.h / 2}
                    x2={frame.x + frame.w}
                    y2={frame.y + frame.h / 2}
                  />
                </g>
              )}

              {guide === "pointer" && pointer && (
                <g stroke={theme.accent.primary} strokeWidth={1} fill="none">
                  <line
                    x1={PHOTO.x}
                    y1={pointer.y}
                    x2={PHOTO.x + PHOTO.w}
                    y2={pointer.y}
                  />
                  <line
                    x1={pointer.x}
                    y1={PHOTO.y}
                    x2={pointer.x}
                    y2={PHOTO.y + PHOTO.h}
                  />
                </g>
              )}

              <rect
                x={frame.x}
                y={frame.y}
                width={frame.w}
                height={frame.h}
                fill="none"
                stroke={theme.text.primary}
                strokeWidth={1}
              />

              {usesCurrentGeometry &&
                [
                  [frame.x, frame.y],
                  [frame.x + frame.w / 2, frame.y],
                  [frame.x + frame.w, frame.y],
                  [frame.x, frame.y + frame.h / 2],
                  [frame.x + frame.w, frame.y + frame.h / 2],
                  [frame.x, frame.y + frame.h],
                  [frame.x + frame.w / 2, frame.y + frame.h],
                  [frame.x + frame.w, frame.y + frame.h],
                ].map(([x, y]) => (
                  <rect
                    key={`${x}-${y}`}
                    x={x - 4}
                    y={y - 4}
                    width={8}
                    height={8}
                    fill={theme.bg.editor}
                    stroke={theme.text.primary}
                  />
                ))}

            </svg>
          </div>
        </Stack>

        <div
          title="Drag to resize the panel"
          onPointerDown={(event: PointEvent) => {
            const startX = event.clientX;
            const startWidth = panelWidth;
            function move(next: PointerEvent) {
              const width = startWidth - (next.clientX - startX);
              setPanelWidth(Math.max(240, Math.min(420, width)));
            }
            function up() {
              window.removeEventListener("pointermove", move);
              window.removeEventListener("pointerup", up);
            }
            window.addEventListener("pointermove", move);
            window.addEventListener("pointerup", up);
          }}
          style={{
            width: 6,
            cursor: "col-resize",
            background: theme.stroke.tertiary,
            flexShrink: 0,
          }}
        />

        <div
          style={{
            width: panelWidth,
            flexShrink: 0,
            overflow: "auto",
            background: theme.bg.elevated,
            padding: 12,
          }}
        >
          <Stack gap={14}>
            <Stack gap={4}>
              <Text size="small" weight="semibold">
                Histogram
              </Text>
              <Histogram />
              <Text size="small" tone="tertiary">
                Framed picture on screen. R, G, and B. Zoom and pan do not
                move this plot.
              </Text>
            </Stack>

            <Stack gap={4}>
              <Text size="small" weight="semibold">
                Guides
              </Text>
              <Button
                variant={guide === "none" ? "ghost" : "secondary"}
                onClick={() =>
                  setGuide((current) =>
                    current === "none"
                      ? "thirds"
                      : current === "thirds"
                        ? "center"
                        : current === "center"
                          ? "pointer"
                          : "none",
                  )
                }
              >
                {guide === "none"
                  ? "None"
                  : guide === "thirds"
                    ? "Thirds"
                    : guide === "center"
                      ? "Center"
                      : "Pointer"}
              </Button>
            </Stack>

            <Stack gap={6}>
              <Text size="small" weight="semibold">
                Orient
              </Text>
              <Row gap={6} wrap>
                <Button variant="secondary">Mirror H</Button>
                <Button variant="secondary">Mirror V</Button>
                <Button variant="secondary">90°</Button>
              </Row>
            </Stack>

            <Stack gap={8}>
              <Text size="small" weight="semibold">
                Light
              </Text>
              <Slider
                label="Exposure"
                value={sliders.exposure}
                min={-5}
                max={5}
                step={0.1}
                format={(value) => `${value.toFixed(1)} ev`}
                onChange={(value) => setSlider("exposure", value)}
              />
              <Slider
                label="Blackpoint"
                value={sliders.blackpoint}
                min={-100}
                max={100}
                step={1}
                onChange={(value) => setSlider("blackpoint", value)}
              />
              <Slider
                label="Shadow"
                value={sliders.shadow}
                min={-100}
                max={100}
                step={1}
                onChange={(value) => setSlider("shadow", value)}
              />
              <Slider
                label="Highlight"
                value={sliders.highlight}
                min={-100}
                max={100}
                step={1}
                onChange={(value) => setSlider("highlight", value)}
              />
              <Slider
                label="Brightness"
                value={sliders.brightness}
                min={-100}
                max={100}
                step={1}
                onChange={(value) => setSlider("brightness", value)}
              />
              <Slider
                label="Contrast"
                value={sliders.contrast}
                min={-100}
                max={100}
                step={1}
                onChange={(value) => setSlider("contrast", value)}
              />
            </Stack>

            <Stack gap={8}>
              <Text size="small" weight="semibold">
                Color
              </Text>
              <Slider
                label="Vibrance"
                value={sliders.vibrance}
                min={-100}
                max={100}
                step={1}
                onChange={(value) => setSlider("vibrance", value)}
              />
              <Slider
                label="Saturation"
                value={sliders.saturation}
                min={-100}
                max={100}
                step={1}
                onChange={(value) => setSlider("saturation", value)}
              />
            </Stack>

            <Stack gap={8}>
              <Text size="small" weight="semibold">
                Detail
              </Text>
              <Slider
                label="Clarity"
                value={sliders.clarity}
                min={-100}
                max={100}
                step={1}
                onChange={(value) => setSlider("clarity", value)}
              />
            </Stack>

            <Stack gap={8}>
              <Text size="small" weight="semibold">
                Frame
              </Text>
              <Slider
                label="Rotation"
                value={rotation}
                min={-45}
                max={45}
                step={0.1}
                format={(value) => `${value.toFixed(1)}°`}
                onChange={(value) => {
                  setRotation(value);
                  editCurrent();
                }}
              />
            </Stack>

            <Slider
              label="JPEG quality"
              value={sliders.quality}
              min={1}
              max={100}
              step={1}
              onChange={(value) => setSlider("quality", value)}
            />

            <Stack gap={4}>
              <Text size="small" weight="semibold">
                View
              </Text>
              <Row gap={4} align="center">
                {VIEWS.map((item) => (
                  <div
                    key={item.id}
                    onPointerDown={(event: PointEvent) => {
                      if (item.needsSave && !hasSave) return;
                      event.currentTarget.setPointerCapture(event.pointerId);
                      setView(item.id);
                    }}
                    onPointerUp={() => setView("current")}
                  >
                    <Button
                      variant={view === item.id ? "primary" : "ghost"}
                      disabled={item.needsSave && !hasSave}
                    >
                      {item.label}
                    </Button>
                  </div>
                ))}
              </Row>
            </Stack>

            <Row gap={6} wrap>
              <Button
                variant="primary"
                onClick={() => {
                  setHasSave(true);
                  setSavedRotation(rotation);
                }}
              >
                Save
              </Button>
              <Button variant="secondary">Save as copy</Button>
              <Button variant="ghost">Exit</Button>
            </Row>
          </Stack>
        </div>
      </div>
    </Stack>
  );
}

function Histogram() {
  const theme = useHostTheme();
  const series = [
    theme.chart.sequence[0],
    theme.chart.sequence[1],
    theme.chart.sequence[2],
  ];
  const centers = [70, 128, 190];
  const widths = [28, 36, 22];
  const heights = [48, 40, 30];

  return (
    <svg viewBox="0 0 256 64" width="100%" height={72}>
      {series.map((color, index) => {
        let path = "";
        for (let x = 0; x <= 256; x += 1) {
          const delta = x - centers[index];
          const y =
            62 -
            heights[index] *
              Math.exp((-delta * delta) / (2 * widths[index] * widths[index]));
          path += `${x === 0 ? "M" : "L"}${x},${y.toFixed(1)}`;
        }
        return (
          <path
            key={color}
            d={path}
            fill="none"
            stroke={color}
            strokeWidth={1.5}
          />
        );
      })}
    </svg>
  );
}

function Slider({
  label,
  value,
  min,
  max,
  step,
  format,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  step: number;
  format?: (value: number) => string;
  onChange: (value: number) => void;
}) {
  const theme = useHostTheme();
  const amount = (value - min) / (max - min);

  function pick(clientX: number, target: HTMLDivElement) {
    const rect = target.getBoundingClientRect();
    const ratio = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
    const raw = min + ratio * (max - min);
    const stepped = Math.round(raw / step) * step;
    const clamped = Math.min(max, Math.max(min, stepped));
    onChange(Number(clamped.toFixed(4)));
  }

  return (
    <Stack gap={2}>
      <Row justify="space-between" align="center">
        <Text size="small" as="span">
          {label}
        </Text>
        <Text size="small" tone="secondary" as="span">
          {format ? format(value) : String(value)}
        </Text>
      </Row>
      <div
        style={{ position: "relative", height: 14, cursor: "pointer" }}
        onPointerDown={(event: PointEvent) => {
          const target = event.currentTarget;
          target.setPointerCapture(event.pointerId);
          pick(event.clientX, target);
        }}
        onPointerMove={(event: PointEvent) => {
          if (event.currentTarget.hasPointerCapture(event.pointerId)) {
            pick(event.clientX, event.currentTarget);
          }
        }}
      >
        <div
          style={{
            position: "absolute",
            top: 6,
            left: 0,
            right: 0,
            height: 2,
            background: theme.stroke.secondary,
          }}
        />
        <div
          style={{
            position: "absolute",
            top: 6,
            left: 0,
            width: `${amount * 100}%`,
            height: 2,
            background: theme.accent.primary,
          }}
        />
        <div
          style={{
            position: "absolute",
            top: 2,
            left: `calc(${amount * 100}% - 5px)`,
            width: 10,
            height: 10,
            background: theme.text.primary,
          }}
        />
      </div>
    </Stack>
  );
}
