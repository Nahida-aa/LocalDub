//! 表单字段卡片: 以 CardX 渲染的 select/input 字段 (与 useAppForm field 桥接)。

import { Input } from "@repo/ui-solid/base/input";
import { CardX } from "@repo/ui-solid/custom/card";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@repo/ui-solid/base/select";

type Option = { value: string; label: string };

/// 空值哨兵: Kobalte 把空字符串当"无选中值" (selectedOption() 为 undefined),
/// 需要用一个非空标记表示"未设置", 提交时再映射回空串。
export const EMPTY = "__none__";

/// form.Field render prop 的 field API 最小面 (value + handleChange)。
export type FieldLike = {
  state: { value: string | undefined };
  handleChange: (v: string) => void;
};

/// 下拉字段卡片 (与 useAppForm field 桥接)。
export function CardSelect(props: {
  title: string;
  description: string;
  field: () => FieldLike;
  options: string[];
  optionLabel?: (v: string) => string;
}) {
  // Solid: JSX 属性需是"调用"才会编译成 getter —— 直接写对象字面量会被静态化,
  // signal 变化时选中项不同步 (React 靠重渲染天然正确, Solid 不行)。
  const labelOf = (v: string): string => (props.optionLabel ? props.optionLabel(v) : v) || "—";
  const selected = (): Option => ({
    value: props.field().state.value || EMPTY,
    label: labelOf(props.field().state.value || ""),
  });
  const options = (): Option[] =>
    props.options.map((v) => ({ value: v || EMPTY, label: labelOf(v) }));

  return (
    <CardX
      title={props.title}
      description={props.description}
      size="sm"
      Footer={
        <Select<Option>
          value={selected()}
          optionValue="value"
          optionTextValue="label"
          onChange={(v) => {
            const raw = v?.value ?? EMPTY;
            props.field().handleChange(raw === EMPTY ? "" : raw);
          }}
          options={options()}
          itemComponent={(p) => <SelectItem item={p.item}>{p.item.rawValue.label}</SelectItem>}
        >
          <SelectTrigger class="w-54">
            <SelectValue<Option>>{(state) => state.selectedOption()?.label ?? "—"}</SelectValue>
          </SelectTrigger>
          <SelectContent />
        </Select>
      }
    />
  );
}

/// 文本字段卡片 (与 useAppForm field 桥接)。
export function CardInput(props: {
  title: string;
  description: string;
  field: () => FieldLike;
  placeholder?: string;
}) {
  return (
    <CardX
      title={props.title}
      description={props.description}
      size="sm"
      Footer={
        <Input
          class="w-80"
          value={props.field().state.value ?? ""}
          placeholder={props.placeholder ?? ""}
          onInput={(e) => props.field().handleChange(e.currentTarget.value)}
        />
      }
    />
  );
}
