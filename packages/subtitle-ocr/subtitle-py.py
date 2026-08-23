import sys, json, os, time
from rapidocr_onnxruntime import RapidOCR

# 强制 stdout 使用 UTF-8，Windows 默认 GBK 会导致中文输出乱码
if sys.stdout.encoding != 'utf-8':
    sys.stdout.reconfigure(encoding='utf-8')

engine = RapidOCR()

def ocr_frame(
	image_path: str,
	bottom_only: bool = True,
	text_score: float | None = None,
	subtitle_only: bool = False,
	providers: list | None = None,
	y_range: tuple | None = None,
) -> tuple[list, float]:
	if not os.path.isfile(image_path):
		raise FileNotFoundError(image_path)

	import cv2, numpy as np
	img = cv2.imdecode(np.fromfile(image_path, dtype=np.uint8), cv2.IMREAD_COLOR)
	if img is None:
		raise ValueError(f"Could not read image: {image_path}")

	h, w = img.shape[:2]

	if y_range:
		# 自定义识别区域 [y_min, y_max]（像素，优先于 bottom_only/subtitle_only 硬编码）
		y_min = max(0, min(h, int(y_range[0])))
		y_max = max(y_min + 1, min(h, int(y_range[1])))
		y_offset = y_min
		roi = img[y_min:y_max, :]
	elif bottom_only:
		y_offset = int(h * 0.6)
		roi = img[y_offset:, :]
	else:
		y_offset = 0
		roi = img

	kwargs = {}
	if text_score is not None:
		kwargs["text_score"] = text_score
	if providers is not None:
		kwargs["providers"] = providers
	t0 = time.perf_counter()
	result, elapse = engine(roi, **kwargs)
	inference_ms = (time.perf_counter() - t0) * 1000
	if not result:
		return [], inference_ms

	lines = []
	for box, text, confidence in result:
		if text.strip():
			conf = float(confidence) if isinstance(confidence, (int, float, str)) else 0.0
			adj_box = [[round(pt[0], 1), round(pt[1] + y_offset, 1)] for pt in box]
			y_center = (adj_box[0][1] + adj_box[2][1]) / 2
			if y_range and not (y_range[0] <= y_center <= y_range[1]):
				continue
			# subtitle_only 的 620-700 硬编码仅适用于默认 bottom_only 模式；
			# 用户显式指定 y_range 时以 y_range 为准，跳过该硬编码过滤
			if subtitle_only and not y_range and not (620 <= y_center <= 700):
				continue
			lines.append({
				"text": text.strip(),
				"confidence": round(conf, 4),
				"box": adj_box,
			})
	return lines, inference_ms


if __name__ == "__main__":
	if len(sys.argv) < 2:
		print(json.dumps({"error": "Usage: python subtitle-py.py <image_path> [--full-frame] [--text-score <float>] [--subtitle-only] [--device cpu|cuda|dml|coreml]"}))
		sys.exit(1)

	image_path = sys.argv[1]
	bottom_only = "--full-frame" not in sys.argv
	text_score = None
	subtitle_only = "--subtitle-only" in sys.argv
	device = "cpu"
	y_range = None
	if "--text-score" in sys.argv:
		idx = sys.argv.index("--text-score")
		if idx + 1 < len(sys.argv):
			text_score = float(sys.argv[idx + 1])
	if "--device" in sys.argv:
		idx = sys.argv.index("--device")
		if idx + 1 < len(sys.argv):
			device = sys.argv[idx + 1]
	if "--y-range" in sys.argv:
		idx = sys.argv.index("--y-range")
		if idx + 2 < len(sys.argv):
			y_range = (float(sys.argv[idx + 1]), float(sys.argv[idx + 2]))

	provider_map = {
		"cuda": ["CUDAExecutionProvider"],
		"dml": ["DmlExecutionProvider"],
		"directml": ["DmlExecutionProvider"],
		"coreml": ["CoreMLExecutionProvider"],
		"rocm": ["ROCMExecutionProvider"],
		"mps": ["CoreMLExecutionProvider"],
		"cpu": ["CPUExecutionProvider"],
	}
	providers = provider_map.get(device, ["CPUExecutionProvider"])

	try:
		lines, inference_ms = ocr_frame(image_path, bottom_only=bottom_only, text_score=text_score, subtitle_only=subtitle_only, providers=providers, y_range=y_range)
		print(json.dumps({"lines": lines, "inference_ms": round(inference_ms, 2)}, ensure_ascii=False))
	except Exception as e:
		print(json.dumps({"error": str(e)}, ensure_ascii=False))
		sys.exit(1)
