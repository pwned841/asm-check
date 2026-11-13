section .text
f:
.spin:
    cmp rax, 10
    jl .spin                ;~
.wait:
    add rbx, 1
    cmp rcx, 5
    jne .wait               ;~
.count:
    inc rax
    cmp rax, 10
    jl .count
.scan:
    inc rsi
    cmp byte [rsi], 0
    jne .scan
.poll:
    cmp byte [flag], 0
    je .poll                ;~
.store:
    mov byte [rdi], 1
    cmp byte [flag], 0
    je .store
.call:
    call f
    cmp rax, 0
    je .call
.down:
    dec rcx
    jnz .down
    ret

section .data
flag db 0
