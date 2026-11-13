section .text
f:
    dec rcx
    test rcx, rcx           ;~
    jz .done
    sub rax, rbx
    cmp rax, 0              ;~
    js .done
    add rax, 1
    test rax, rax
    jb .done
    mov rax, rbx
    test rax, rax
    jz .done
    and rax, 1
    test rbx, rbx
    jz .done
.done:
    ret
