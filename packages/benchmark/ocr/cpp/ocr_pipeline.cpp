#include <iostream>
#include <fstream>
#include <string>
#include <vector>
#include <algorithm>
#include <cmath>
#include <cstring>
#include <chrono>
#include <sstream>
#include <iomanip>

#ifndef M_PI_2
#define M_PI_2 1.57079632679489661923
#endif

#include "onnxruntime_cxx_api.h"
#include "image.h"
#include "geometry.h"

// --- Constants ---
constexpr int DET_LIMIT_SIDE = 736;
constexpr int CLS_H = 48;
constexpr int CLS_W = 192;
constexpr int REC_H = 48;
constexpr int REC_MAX_W = 320;

constexpr float DET_THRESH = 0.3f;

constexpr float UNCLIP_RATIO = 1.6f;
constexpr int MAX_CANDIDATES = 1000;

struct OCRResult {
    std::string text;
    struct Segment {
        std::string text;
        float confidence;
        std::vector<std::vector<int>> box; // 4x2
    };
    std::vector<Segment> segments;
    double detMs, postMs, recMs, totalMs;
};

// --- Load char list from JSON ---
static std::vector<std::string> loadCharList(const std::string& path) {
    std::ifstream f(path);
    if (!f) return {};
    std::string json((std::istreambuf_iterator<char>(f)), std::istreambuf_iterator<char>());
    std::vector<std::string> chars;
    // Simple JSON array parser for ["", "char1", "char2", ...]
    size_t pos = 0;
    while (pos < json.size() && json[pos] != '[') pos++;
    if (pos >= json.size()) return chars;
    pos++; // skip '['
    while (pos < json.size() && json[pos] != ']') {
        // Skip whitespace/comma
        while (pos < json.size() && (json[pos] == ' ' || json[pos] == '\n' || json[pos] == '\r' || json[pos] == '\t' || json[pos] == ','))
            pos++;
        if (pos >= json.size() || json[pos] == ']') break;
        if (json[pos] == '"') {
            pos++; // skip opening quote
            std::string ch;
            while (pos < json.size() && json[pos] != '"') {
                if (json[pos] == '\\') {
                    pos++;
                    if (pos < json.size()) {
                        if (json[pos] == 'u') {
                            // Unicode escape \uXXXX - simplified
                            ch += '?';
                            for (int k = 0; k < 5 && pos < json.size(); k++) pos++;
                        } else {
                            ch += json[pos++];
                        }
                    }
                } else {
                    ch += json[pos++];
                }
            }
            if (pos < json.size()) pos++; // skip closing quote
            chars.push_back(ch);
        } else break;
    }
    return chars;
}

// --- CTC decode ---
struct CTCResult { std::string text; float confidence; };
static CTCResult ctcDecode(const float* logits, int timesteps, int numClasses, const std::vector<std::string>& charList) {
    std::string chars;
    std::vector<float> confs;
    int prev = -1; // -1 = blank
    for (int t = 0; t < timesteps; ++t) {
        const float* row = logits + t * numClasses;
        int maxIdx = 0;
        float maxVal = row[0];
        for (int c = 1; c < numClasses; ++c) {
            if (row[c] > maxVal) { maxVal = row[c]; maxIdx = c; }
        }
        if (maxIdx == 0) { prev = -1; continue; } // blank
        if (maxIdx != prev) {
            if (maxIdx < (int)charList.size()) {
                chars += charList[maxIdx];
                confs.push_back(maxVal);
            }
        }
        prev = maxIdx;
    }
    float avgConf = 0;
    for (float c : confs) avgConf += c;
    if (!confs.empty()) avgConf /= confs.size();
    return {chars, avgConf};
}

// --- Preprocess for detection ---
struct DetPreproc {
    std::vector<float> tensor; // NCHW
    int origH, origW, resizedH, resizedW;
};

static DetPreproc preprocessDet(const uint8_t* rgb, int H, int W) {
    DetPreproc out;
    out.origH = H; out.origW = W;

    int newW, newH;
    if (H <= W) {
        newH = DET_LIMIT_SIDE;
        newW = (int)std::round((float)W * DET_LIMIT_SIDE / H);
    } else {
        newW = DET_LIMIT_SIDE;
        newH = (int)std::round((float)H * DET_LIMIT_SIDE / W);
    }
    newW = ((newW + 31) / 32) * 32;
    newH = ((newH + 31) / 32) * 32;
    out.resizedW = newW; out.resizedH = newH;

    std::vector<uint8_t> resized(newW * newH * 3);
    resizeBilinear(rgb, W, H, resized.data(), newW, newH);

    out.tensor.resize(3 * newH * newW);
    const float mean[3] = {0.485f, 0.456f, 0.406f};
    const float std[3]  = {0.229f, 0.224f, 0.225f};
    for (int y = 0; y < newH; ++y) {
        for (int x = 0; x < newW; ++x) {
            for (int c = 0; c < 3; ++c) {
                float pixel = resized[(y * newW + x) * 3 + c] / 255.0f;
                out.tensor[c * newH * newW + y * newW + x] = (pixel - mean[c]) / std[c];
            }
        }
    }
    return out;
}

// --- Preprocess for classification ---
static std::vector<float> preprocessCls(const uint8_t* rgb, int H, int W) {
    std::vector<uint8_t> resized(CLS_W * CLS_H * 3);
    resizeBilinear(rgb, W, H, resized.data(), CLS_W, CLS_H);

    std::vector<float> tensor(3 * CLS_H * CLS_W);
    for (int y = 0; y < CLS_H; ++y) {
        for (int x = 0; x < CLS_W; ++x) {
            for (int c = 0; c < 3; ++c) {
                float pixel = resized[(y * CLS_W + x) * 3 + c] / 255.0f;
                tensor[c * CLS_H * CLS_W + y * CLS_W + x] = (pixel - 0.5f) / 0.5f;
            }
        }
    }
    return tensor;
}

// --- Preprocess for recognition ---
struct RecPreproc {
    std::vector<float> tensor;
    int width;
};

static RecPreproc preprocessRec(const uint8_t* rgb, int H, int W) {
    (void)H;
    float whRatio = (float)W / REC_H;
    int imgW = std::min(REC_MAX_W, std::max(32, (int)std::round(REC_H * whRatio)));

    std::vector<uint8_t> resized(imgW * REC_H * 3);
    resizeBilinear(rgb, W, REC_H, resized.data(), imgW, REC_H);

    RecPreproc out;
    out.width = imgW;
    out.tensor.resize(3 * REC_H * imgW);
    for (int y = 0; y < REC_H; ++y) {
        for (int x = 0; x < imgW; ++x) {
            for (int c = 0; c < 3; ++c) {
                float pixel = resized[(y * imgW + x) * 3 + c] / 255.0f;
                out.tensor[c * REC_H * imgW + y * imgW + x] = (pixel - 0.5f) / 0.5f;
            }
        }
    }
    return out;
}

// --- DB post-processing (complete, no Python dependency) ---
static std::vector<std::pair<Polygon,float>> dbPostprocess(
    const float* heatmap, int H, int W, int origH, int origW,
    float thresh, float boxThresh, float unclipRatio, int maxCandidates)
{
    float hmax = heatmap[0];
    for (int i = 1; i < H * W; ++i) hmax = std::max(hmax, heatmap[i]);

    std::vector<float> prob(H * W);
    if (hmax > 1.0f) {
        for (int i = 0; i < H * W; ++i)
            prob[i] = 1.0f / (1.0f + std::exp(-heatmap[i]));
    } else {
        std::copy(heatmap, heatmap + H * W, prob.begin());
    }

    std::vector<uint8_t> bitmap(H * W);
    for (int i = 0; i < H * W; ++i)
        bitmap[i] = prob[i] > thresh ? 1 : 0;

    // 2x2 dilation before connectedComponents — matches Python use_dilation:true
    std::vector<uint8_t> dilated(H * W, 0);
    for (int y = 0; y < H; ++y) {
        for (int x = 0; x < W; ++x) {
            if (bitmap[y * W + x]) {
                dilated[y * W + x] = 1;
                if (x + 1 < W) dilated[y * W + x + 1] = 1;
                if (y + 1 < H) dilated[(y + 1) * W + x] = 1;
                if (x + 1 < W && y + 1 < H) dilated[(y + 1) * W + x + 1] = 1;
            }
        }
    }
    auto components = connectedComponents(dilated.data(), H, W);
    std::vector<std::pair<Polygon,float>> results;

    int nComp = std::min((int)components.size(), maxCandidates);
    for (int i = 0; i < nComp; ++i) {
        auto& comp = components[i];
        if (comp.empty()) continue;

        Polygon contour = traceContour(comp, H, W);
        if (contour.size() < 4) continue;

        auto rect = minAreaRect(contour);
        Polygon boxPts = boxPoints(rect);
        float sside = std::min(rect.width, rect.height);
        if (sside < 3) continue;

        Polygon boxPtsForScore = boxPts;
        float score = boxScore(prob.data(), H, W, boxPtsForScore);
        if (score < boxThresh) continue;

        // Expand rotated rect uniformly (pyclipper-equivalent for rectangles)
        float dist = rect.width * rect.height * unclipRatio / (2.0f * (rect.width + rect.height));
        dist = std::max(3.0f, dist);
        rect.width += 2.0f * dist;
        rect.height += 2.0f * dist;
        Polygon boxFinal = boxPoints(rect);
        {
            float sside2 = std::min(rect.width, rect.height);
            if (sside2 < 5) continue;
        }

        for (auto& p : boxFinal) {
            p.x = std::round(p.x / W * origW);
            p.y = std::round(p.y / H * origH);
            p.x = std::max(0.0f, std::min((float)origW - 1, p.x));
            p.y = std::max(0.0f, std::min((float)origH - 1, p.y));
        }

        results.push_back({boxFinal, score});
    }

    return results;
}

// --- Warp rotated crop using RotatedRect (affine rotation + bilinear sampling) ---
static Image warpRotatedCrop(const Image& img, const RotatedRect& rect) {
    int dstW = std::max(1, (int)std::round(rect.width));
    int dstH = std::max(1, (int)std::round(rect.height));
    if (dstW > img.w * 2) dstW = std::min(dstW, img.w);
    if (dstH > img.h * 2) dstH = std::min(dstH, img.h);

    Image out;
    out.w = dstW; out.h = dstH; out.c = img.c;
    out.data.resize(dstW * dstH * img.c, 0);

    float cosA = std::cos(rect.angle), sinA = std::sin(rect.angle);
    float cx = rect.center.x, cy = rect.center.y;

    for (int dy = 0; dy < dstH; ++dy) {
        float perp = dy - dstH / 2.0f;
        for (int dx = 0; dx < dstW; ++dx) {
            float proj = dx - dstW / 2.0f;
            float sx = proj * cosA - perp * sinA + cx;
            float sy = proj * sinA + perp * cosA + cy;
            for (int c = 0; c < img.c; ++c)
                out.data[(dy * dstW + dx) * img.c + c] = (uint8_t)std::round(sampleBilinear(img, sx, sy, c));
        }
    }
    return out;
}

// --- Rotate image 180° ---
static Image rotate180(const Image& img) {
    Image out;
    out.w = img.w; out.h = img.h; out.c = img.c;
    out.data.resize(img.data.size());
    for (int y = 0; y < img.h; ++y) {
        for (int x = 0; x < img.w; ++x) {
            int srcIdx = (y * img.w + x) * 3;
            int dstIdx = ((img.h - 1 - y) * img.w + (img.w - 1 - x)) * 3;
            out.data[dstIdx] = img.data[srcIdx];
            out.data[dstIdx+1] = img.data[srcIdx+1];
            out.data[dstIdx+2] = img.data[srcIdx+2];
        }
    }
    return out;
}

// --- Run full OCR pipeline ---
static OCRResult runOcr(
    const std::string& imagePath,
    const std::vector<std::string>& charList,
    Ort::Session& detSession,
    Ort::Session& clsSession,
    Ort::Session& recSession,
    Ort::MemoryInfo& memInfo,
    float textScore = 0.5f,
    bool subtitleOnly = false)
{
    using clock = std::chrono::high_resolution_clock;
    auto tStart = clock::now();

    if (subtitleOnly && textScore > 0.3f) textScore = 0.3f;

    OCRResult result;
    result.detMs = result.postMs = result.recMs = 0;

    // Load image
    auto img = loadImage(imagePath.c_str());

    // --- Detection ---
    auto t0 = clock::now();
    auto detPrep = preprocessDet(img.data.data(), img.h, img.w);

    // Det inference
    std::vector<int64_t> detShape = {1, 3, detPrep.resizedH, detPrep.resizedW};
    Ort::Value detInput = Ort::Value::CreateTensor<float>(
        memInfo, detPrep.tensor.data(), detPrep.tensor.size(), detShape.data(), detShape.size());

    auto detInNames = detSession.GetInputNames();
    auto detOutNames = detSession.GetOutputNames();
    std::vector<const char*> detInputNames = {detInNames[0].c_str()};
    std::vector<const char*> detOutputNames = {detOutNames[0].c_str()};

    auto detOut = detSession.Run(Ort::RunOptions{nullptr}, detInputNames.data(), &detInput, 1,
                                 detOutputNames.data(), 1);
    float* heatmapData = detOut[0].GetTensorMutableData<float>();
    auto detShapeOut = detOut[0].GetTensorTypeAndShapeInfo().GetShape();
    int outH = (int)detShapeOut[2], outW = (int)detShapeOut[3];

    auto t1 = clock::now();
    result.detMs = std::chrono::duration<double, std::milli>(t1 - t0).count();

    // --- DB post-process ---
    auto boxesPts = dbPostprocess(heatmapData, outH, outW, img.h, img.w,
                                  DET_THRESH, textScore, UNCLIP_RATIO, MAX_CANDIDATES);

    auto t2 = clock::now();
    result.postMs = std::chrono::duration<double, std::milli>(t2 - t1).count();

    // --- Recognition for each box ---
    for (auto& [boxPts, score] : boxesPts) {
        (void)score;
        if (boxPts.size() < 4) continue;

        // Y-position filter for subtitle only
        if (subtitleOnly) {
            float ymin = boxPts[0].y, ymax = boxPts[0].y;
            for (auto& p : boxPts) {
                ymin = std::min(ymin, p.y);
                ymax = std::max(ymax, p.y);
            }
            float yCenter = (ymin + ymax) / 2;
            if (yCenter < 620 || yCenter > 700) continue;
        }

        // Determine orientation from minAreaRect
        auto rotRect = minAreaRect(boxPts);
        Image crop = warpRotatedCrop(img, rotRect);
        if (crop.w < 4 || crop.h < 4) continue;

        // Cls inference
        auto clsTensor = preprocessCls(crop.data.data(), crop.h, crop.w);
        std::vector<int64_t> clsShape = {1, 3, CLS_H, CLS_W};
        Ort::Value clsInputVal = Ort::Value::CreateTensor<float>(
            memInfo, clsTensor.data(), clsTensor.size(), clsShape.data(), clsShape.size());

        auto clsInNames = clsSession.GetInputNames();
        auto clsOutNames = clsSession.GetOutputNames();
        std::vector<const char*> clsInputNames = {clsInNames[0].c_str()};
        std::vector<const char*> clsOutputNames = {clsOutNames[0].c_str()};

        auto clsOut = clsSession.Run(Ort::RunOptions{nullptr}, clsInputNames.data(), &clsInputVal, 1,
                                     clsOutputNames.data(), 1);
        float* clsData = clsOut[0].GetTensorMutableData<float>();
        bool rotate = clsData[0] < clsData[1]; // clsData[0] = "0°" prob, clsData[1] = "180°" prob

        // Rotate if needed
        Image recCrop = rotate ? rotate180(crop) : crop;

        // Rec inference
        auto recPrep = preprocessRec(recCrop.data.data(), recCrop.h, recCrop.w);
        auto t3 = clock::now();

        std::vector<int64_t> recShape = {1, 3, REC_H, recPrep.width};
        Ort::Value recInputVal = Ort::Value::CreateTensor<float>(
            memInfo, recPrep.tensor.data(), recPrep.tensor.size(), recShape.data(), recShape.size());

        auto recInNames = recSession.GetInputNames();
        auto recOutNames = recSession.GetOutputNames();
        std::vector<const char*> recInputNames = {recInNames[0].c_str()};
        std::vector<const char*> recOutputNames = {recOutNames[0].c_str()};

        auto recOut = recSession.Run(Ort::RunOptions{nullptr}, recInputNames.data(), &recInputVal, 1,
                                     recOutputNames.data(), 1);

        auto t4 = clock::now();
        result.recMs += std::chrono::duration<double, std::milli>(t4 - t3).count();

        float* recData = recOut[0].GetTensorMutableData<float>();
        auto recShapeOut = recOut[0].GetTensorTypeAndShapeInfo().GetShape();
        int timesteps = (int)recShapeOut[1];
        int numClasses = (int)recShapeOut[2];

        auto [text, conf] = ctcDecode(recData, timesteps, numClasses, charList);
        if (text.empty()) continue;
        if (text.empty()) continue;

        // Convert box to integer coordinates for output
        std::vector<std::vector<int>> boxOut;
        for (auto& p : boxPts) {
            boxOut.push_back({(int)std::round(p.x), (int)std::round(p.y)});
        }

        result.segments.push_back({text, conf, std::move(boxOut)});
    }

    // Sort top-to-bottom, left-to-right
    std::sort(result.segments.begin(), result.segments.end(),
        [](auto& a, auto& b) {
            auto boxCenterY = [](auto& s) {
                float sum = 0;
                for (auto& p : s.box) sum += p[1];
                return sum / s.box.size();
            };
            auto boxCenterX = [](auto& s) {
                float sum = 0;
                for (auto& p : s.box) sum += p[0];
                return sum / s.box.size();
            };
            float ya = boxCenterY(a), yb = boxCenterY(b);
            if (std::abs(ya - yb) > 20) return ya < yb;
            return boxCenterX(a) < boxCenterX(b);
        });

    // Build full text
    for (auto& seg : result.segments)
        result.text += seg.text;

    auto tEnd = clock::now();
    result.totalMs = std::chrono::duration<double, std::milli>(tEnd - tStart).count();

    return result;
}

// --- JSON output ---
static std::string toJson(const OCRResult& r) {
    std::ostringstream ss;
    ss << std::fixed << std::setprecision(2);
    ss << "{\n";
    ss << "  \"text\": " << std::quoted(r.text) << ",\n";
    ss << "  \"segments\": [\n";
    for (size_t i = 0; i < r.segments.size(); ++i) {
        auto& seg = r.segments[i];
        ss << "    {\"text\": " << std::quoted(seg.text)
           << ", \"confidence\": " << seg.confidence
           << ", \"box\": [";
        for (size_t j = 0; j < seg.box.size(); ++j) {
            ss << "[" << seg.box[j][0] << "," << seg.box[j][1] << "]";
            if (j + 1 < seg.box.size()) ss << ",";
        }
        ss << "]}";
        if (i + 1 < r.segments.size()) ss << ",";
        ss << "\n";
    }
    ss << "  ],\n";
    ss << "  \"detInferenceMs\": " << r.detMs << ",\n";
    ss << "  \"postprocessMs\": " << r.postMs << ",\n";
    ss << "  \"recInferenceMs\": " << r.recMs << ",\n";
    ss << "  \"totalMs\": " << r.totalMs << "\n";
    ss << "}\n";
    return ss.str();
}

int main(int argc, char* argv[]) {
    if (argc < 2) {
        std::cerr << "Usage: " << argv[0] << " <image_path> [text_score] [--subtitle-only]" << std::endl;
        return 1;
    }

    std::string imagePath = argv[1];
    float textScore = 0.5f;
    bool subtitleOnly = false;
    for (int i = 2; i < argc; ++i) {
        if (strcmp(argv[i], "--subtitle-only") == 0) subtitleOnly = true;
        else if (argv[i][0] != '-') textScore = std::stof(argv[i]);
    }

    try {
        // Find model directory relative to executable
        std::string modelDir;
        const char* envModels = std::getenv("OCR_MODELS_DIR");
        if (envModels) {
            modelDir = envModels;
        } else {
            // Default: relative to venv
            modelDir = std::string(std::getenv("HOME")) + "/repos/env_ls/LocalDub/.venv/lib/python3.14/site-packages/rapidocr_onnxruntime/models";
        }

        // Find char list
        std::string keysPath;
        const char* envKeys = std::getenv("OCR_KEYS_PATH");
        if (envKeys) {
            keysPath = envKeys;
        } else {
            // Default: same dir as this binary, or relative path
            keysPath = std::string(std::getenv("HOME")) + "/repos/env_ls/LocalDub/packages/benchmark/ocr/compute/ppocr_keys.json";
        }

        auto charList = loadCharList(keysPath);
        if (charList.empty()) {
            std::cerr << "Failed to load char list from " << keysPath << std::endl;
            return 1;
        }

        // Initialize ORT
        Ort::Env env(ORT_LOGGING_LEVEL_WARNING, "ocr");
        Ort::SessionOptions sessionOptions;
        sessionOptions.SetIntraOpNumThreads(4);
        sessionOptions.SetGraphOptimizationLevel(GraphOptimizationLevel::ORT_ENABLE_ALL);

        // Increase session timeout for model loading
        sessionOptions.SetExecutionMode(ExecutionMode::ORT_SEQUENTIAL);

        auto memInfo = Ort::MemoryInfo::CreateCpu(OrtArenaAllocator, OrtMemTypeDefault);

        // Load models
        auto detPath = modelDir + "/ch_PP-OCRv3_det_infer.onnx";
        auto clsPath = modelDir + "/ch_ppocr_mobile_v2.0_cls_infer.onnx";
        auto recPath = modelDir + "/ch_PP-OCRv3_rec_infer.onnx";

        Ort::Session detSession(env, detPath.c_str(), sessionOptions);
        Ort::Session clsSession(env, clsPath.c_str(), sessionOptions);
        Ort::Session recSession(env, recPath.c_str(), sessionOptions);

        // Run OCR
        auto result = runOcr(imagePath, charList, detSession, clsSession, recSession, memInfo,
                             textScore, subtitleOnly);

        // Output JSON
        std::cout << toJson(result);

    } catch (const std::exception& e) {
        std::cerr << "Error: " << e.what() << std::endl;
        return 1;
    }

    return 0;
}
