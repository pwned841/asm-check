#!/usr/bin/env python3
"""Compares asm-check with NASM on generated instructions.

Every mnemonic is combined with registers of each size, memory operands with
and without a size, immediates and labels. NASM assembles the result and
asm-check checks it; a line NASM rejects that asm-check passes is a missed
error, and the reverse is a false positive. Needs Docker and the image from
scripts/Dockerfile, or a local `nasm`.

    python3 scripts/nasm-diff.py
"""
import collections, json, os, re, shutil, subprocess, sys, tempfile

HEAD = "section .data\nlbl dq 0\nsection .text\nf:\n"
REGS = ["rax", "eax", "ax", "al", "rcx", "cl", "r8", "r8d", "r8w", "r8b", "rsp", "sil"]
MEMS = ["[rdi]", "byte [rdi]", "word [rdi]", "dword [rdi]", "qword [rdi]", "[lbl]",
        "dword [lbl]", "[rdi+rcx*4]"]
IMMS = ["1", "300", "0x100000000", "lbl"]
TWO = ["mov", "add", "sub", "and", "or", "xor", "cmp", "test", "adc", "sbb", "xchg", "imul",
       "movzx", "movsx", "movsxd", "lea", "cmovz", "shl", "shr", "sar", "rol", "ror", "bt",
       "bts", "bsf", "popcnt", "xadd", "cmpxchg"]
ONE = ["inc", "dec", "neg", "not", "push", "pop", "mul", "div", "idiv", "imul", "call", "jmp",
       "setz", "bswap"]
ADDRS = ["[bx]", "[eax]", "[rax+ebx]", "[rdi+rcx*3]", "[rax*3]", "[rax+rbx+rcx]", "[rsp*2]",
         "[rax+rsp*2]", "[rcx*8+rdx*4]", "[rdi+rcx*8+16]", "[ax]", "[cl]", "[eax+ecx*4]",
         "[rel lbl]", "[lbl+rax]", "[r8+r9*2]", "[rax*5]", "[rax*9]", "[rdi-8]", "[rbx+rbx]",
         "[rsp+rsp]", "[rsp+rax*8]", "[rax+rsp]", "[r12]", "[r13]", "[rbp]", "[di]"]
VALUES = ["0", "1", "127", "128", "255", "256", "-1", "-128", "-129", "-256", "-257", "32767",
          "32768", "65535", "65536", "-32768", "-32769", "-65537", "0x7FFFFFFF", "0x80000000",
          "0xFFFFFFFF", "0x100000000", "-0x80000000", "-0x80000001", "0x7FFFFFFFFFFFFFFF",
          "0xFFFFFFFFFFFFFFFF"]
DESTS = ["al", "ax", "eax", "rax", "r8b", "r8w", "r8d", "r8", "byte [rdi]", "word [rdi]",
         "dword [rdi]", "qword [rdi]"]
# NASM reports a class of errors (bad addresses, high-byte registers with REX) before it
# looks at operand combinations, and warns about immediates only when the file has no
# error: each kind of check therefore gets a file of its own.
OPERAND_LINTS = {"operand-size-mismatch", "ambiguous-operand-size", "invalid-operands",
                 "invalid-address", "unknown-instruction"}


def cases():
    ops = REGS + MEMS
    operand_cases = [f"{m} {d}, {s}" for m in TWO for d in ops for s in ops + IMMS]
    operand_cases += [f"{m} {d}" for m in ONE for d in ops + IMMS]
    address_cases = [f"{m} {r}, {a}" for a in ADDRS
                     for m, r in [("mov", "rax"), ("lea", "rax"), ("mov", "eax"), ("mov", "al")]]
    immediate_cases = [f"{m} {d}, {v}" for m in ["mov", "add", "sub", "and", "or", "xor", "cmp",
                       "test", "adc", "sbb"] for d in DESTS for v in VALUES]
    return {"operands": (HEAD, operand_cases), "addresses": (HEAD, address_cases),
            "immediates": ("section .text\nf:\n", immediate_cases)}


def assemble(directory):
    script = "for f in *.s; do nasm -f elf64 -o /dev/null $f 2> $f.err; done; true"
    if shutil.which("nasm") and b"2.16" in subprocess.run(["nasm", "-v"], capture_output=True).stdout:
        subprocess.run(["sh", "-c", script], cwd=directory, check=True)
    else:
        subprocess.run(["docker", "run", "--rm", "-v", f"{directory}:/w", "-w", "/w",
                        "asmcheck-nasm", "sh", "-c", script], check=True, capture_output=True)


def main():
    binary = os.environ.get("ASM_CHECK", "asm-check")
    failures = 0
    with tempfile.TemporaryDirectory() as directory:
        suites = cases()
        for name, (head, lines) in suites.items():
            with open(f"{directory}/{name}.s", "w") as out:
                out.write(head + "\n".join("    " + line for line in lines) + "\n")
        assemble(directory)
        pattern = re.compile(r"^.*?:(\d+): (error|warning): (.*)$")
        for name, (head, lines) in suites.items():
            first = head.count("\n") + 1
            rejected, warned = {}, {}
            for text in open(f"{directory}/{name}.s.err").read().splitlines():
                match = pattern.match(text)
                if match:
                    (rejected if match[2] == "error" else warned).setdefault(int(match[1]), match[3])
            found = subprocess.run([binary, "--format", "json", "--pedantic", f"{directory}/{name}.s"],
                                   capture_output=True, text=True).stdout
            flagged = collections.defaultdict(set)
            for item in json.loads(found or "[]"):
                wanted = {"immediate-out-of-range"} if name == "immediates" else OPERAND_LINTS
                if item["lint"] in wanted:
                    flagged[item["line"]].add(item["lint"])
            false_positives, missed = [], []
            for index, line in enumerate(lines):
                at = first + index
                nasm = at in rejected or (name == "immediates" and "exceeds bounds" in warned.get(at, ""))
                ours = bool(flagged.get(at))
                if ours and not nasm:
                    false_positives.append(line)
                elif nasm and not ours:
                    missed.append(line)
            print(f"{name:11} {len(lines):6} cases  agree {len(lines) - len(false_positives) - len(missed):6}"
                  f"  false positives {len(false_positives)}  missed {len(missed)}")
            for line in false_positives[:5]:
                print("   false positive:", line)
            for line in missed[:5]:
                print("   missed:        ", line)
            failures += len(false_positives) + len(missed)
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
