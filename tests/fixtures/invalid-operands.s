section .text
operands:
    mov [first], [second]           ;~
    cmp byte [rdi], [rsi]           ;~
    mov counter, rax                ;~
    mov 5, rax                      ;~
    cmp 5, rax                      ;~
    inc 5                           ;~
    lea rax, rbx                    ;~
    lea rax, [rbx]
    push eax                        ;~
    pop bl                          ;~
    push rax
    push ax
    and rax, 0xFFFFFFFF             ;~
    and eax, 0xFFFFFFFF
    cmp rax, 0x7FFFFFFF
    add rax, 0x100000000            ;~
    mov rax, 0x100000000
    mov qword [rdi], 0x100000000    ;~
    and rax, 0xFFFFFFFFFFFFFFFF
    add rax, -1
    mov rax                         ;~
    syscall rax                     ;~
    ret 8

section .data
first dq 0
second dq 0
counter dq 0
