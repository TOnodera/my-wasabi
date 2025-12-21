extern crate alloc;

use crate::error;
use crate::info;
use crate::result::Result;
use alloc::boxed::Box;
use core::arch::asm;
use core::arch::global_asm;
use core::fmt;
use core::fmt::Display;
use core::marker::PhantomData;
use core::mem::offset_of;
use core::mem::size_of;
use core::mem::size_of_val;
use core::panic;
use core::pin::Pin;

pub fn hlt() {
    unsafe { asm!("hlt") }
}

pub fn busy_loop_hint() {
    unsafe { asm!("pause") }
}

pub fn read_io_port_u8(port: u16) -> u8 {
    let mut data: u8;
    unsafe {
        asm!("in al, dx"
            , out("al") data
            , in("dx") port
        )
    }
    data
}

pub fn write_io_port_u8(port: u16, data: u8) {
    unsafe {
        asm!("out dx, al"
            , in("al") data
            , in("dx") port
        )
    }
}

pub fn read_cr3() -> *mut PML4 {
    let mut cr3: *mut PML4;
    unsafe {
        asm!("mov rax, cr3",
        out("rax") cr3
        )
    }
    cr3
}

pub const PAGE_SIZE: usize = 4096;
const ATTR_MASK: u64 = 0xFFF;
const ATTR_PRESENT: u64 = 1 << 0;
const ATTR_WRITABLE: u64 = 1 << 1;
const ATTR_WRITE_THROUGH: u64 = 1 << 3;
const ATTR_CACHE_DISABLE: u64 = 1 << 4;

#[derive(Debug, Copy, Clone)]
#[repr(u64)]
pub enum PageAttr {
    NotPresent = 0,
    ReadWriteKernel = ATTR_PRESENT | ATTR_WRITABLE,
    ReadWriteIo =
        ATTR_PRESENT | ATTR_WRITABLE | ATTR_WRITE_THROUGH | ATTR_CACHE_DISABLE,
}

#[derive(Debug, Eq, PartialEq)]
pub enum TranslationResult {
    PageMapped4K { phys: u64 },
    PageMapped2M { phys: u64 },
    PageMapped1G { phys: u64 },
}

#[repr(transparent)]
pub struct Entry<const LEVEL: usize, const SHIFT: usize, Next> {
    value: u64,
    _marker: PhantomData<Next>,
}
impl<const LEVEL: usize, const SHIFT: usize, NEXT> Entry<LEVEL, SHIFT, NEXT> {
    fn read_value(&self) -> u64 {
        self.value
    }
    fn is_present(&self) -> bool {
        (self.read_value() & (1 << 0)) != 0
    }
    fn is_writable(&self) -> bool {
        (self.read_value() & (1 << 1)) != 0
    }
    fn is_user(&self) -> bool {
        (self.read_value() & (1 << 2)) != 0
    }
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "L{}ENTRY @ {:#p} {{ {:#018X} {}{}{} ",
            LEVEL,
            self,
            self.read_value(),
            if self.is_present() { "P" } else { "N" },
            if self.is_writable() { "W" } else { "R" },
            if self.is_user() { "U" } else { "S" },
        )?;
        write!(f, "}}")
    }
    fn table(&self) -> Result<&NEXT> {
        if self.is_present() {
            Ok(unsafe { &*((self.value & !ATTR_MASK) as *const NEXT) })
        } else {
            Err("Page not found.")
        }
    }
}
impl<const LEVEL: usize, const SHIFT: usize, NEXT> fmt::Display
    for Entry<LEVEL, SHIFT, NEXT>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.format(f)
    }
}
impl<const LEVEL: usize, const SHIFT: usize, NEXT> fmt::Debug
    for Entry<LEVEL, SHIFT, NEXT>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.format(f)
    }
}

#[repr(align(4096))]
pub struct Tabel<const LEVEL: usize, const SHIFT: usize, NEXT> {
    entry: [Entry<LEVEL, SHIFT, NEXT>; 512],
}
impl<const LEVEL: usize, const SHIFT: usize, NEXT> Tabel<LEVEL, SHIFT, NEXT> {
    fn format(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "L{}TABLE @ {:#p} {{", LEVEL, self)?;
        for i in 0..512 {
            let e = &self.entry[i];
            if !e.is_present() {
                continue;
            }
            writeln!(f, "  entry[{:3}] = {:?}", i, e)?;
        }
        writeln!(f, "}}")
    }
    pub fn next_level(&self, index: usize) -> Option<&NEXT> {
        self.entry.get(index).and_then(|e| e.table().ok())
    }
}
impl<const LEVEL: usize, const SHIFT: usize, NEXT> fmt::Debug
    for Tabel<LEVEL, SHIFT, NEXT>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.format(f)
    }
}

pub type PT = Tabel<1, 12, [u8; PAGE_SIZE]>;
pub type PD = Tabel<2, 21, PT>;
pub type PDPT = Tabel<3, 30, PD>;
pub type PML4 = Tabel<4, 39, PDPT>;

// es(Extra Segment) レジスタにセグメントセレクタを書き込む
pub unsafe fn write_es(selector: u16) {
    /**
     * x86 アーキテクチャでは、セグメントレジスタ（CS、DS、ES、FS、GS、
     * SS）は それぞれ特定の目的で使用されます。ES
     * レジスタは、主に文字列操作命令で
     * 使用される追加のデータセグメントを指すために使われます。
     */
    asm!("mov es, ax", in("ax") selector)
}

pub unsafe fn write_cs(cs: u16) {
    asm!(
        // ripレジスタには次に実行される命令のアドレスが入ってる。2fあ2つ後の命令の意味
        // 戻ってきたときに実行する命令を汎用レジスタに保存
        "lea rax, [rip + 2f]",
        // CS(コードセグメント)をスタックに積む
        "push cx",
        // 戻り先をスタックに積む
        "push rax",
        // ここでcsをcxの値に、ripをraxの値に書き換えてジャンプする
        "ljmp [rsp]",
        // ljmp [rsp]はスタックをポップしないpushした分を戻している
        "2:",
        "add rsp,  8 + 2",
        //
        in("cx") cs
    )
}

pub unsafe fn write_ss(selector: u16) {
    // スタックセグメントレジスタを設定
    asm!("mov ss, ax", in("ax") selector)
}

pub unsafe fn write_ds(selector: u16) {
    // データセグメントレジスタを設定
    asm!("mov ds, ax", in("ax") selector)
}

pub unsafe fn write_fs(selector: u16) {
    // FSセグメントレジスタを設定
    asm!("mov fs, ax", in("ax") selector)
}

pub unsafe fn write_gs(selector: u16) {
    // GSセグメントレジスタを設定え
    asm!("mov gs, ax", in("ax") selector)
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct FPUContext {
    data: [u8; 512],
}

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct GeneralRegisterContext {
    rax: u64,
    rdx: u64,
    rbx: u64,
    rbp: u64,
    rsi: u64,
    rdi: u64,
    r8: u64,
    r9: u64,
    r10: u64,
    r11: u64,
    r12: u64,
    r13: u64,
    r14: u64,
    r15: u64,
    rcx: u64,
}
const _: () = assert!(size_of::<GeneralRegisterContext>() == (16 - 1) * 8);

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct InterruptContext {
    rip: u64,    // 次の命令が実行されるアドレス
    cs: u64,     // コードセグメント
    rflags: u64, // フラグレジスタ
    rsp: u64,    // スタックポインタ
    ss: u64,     // スタックセグメント
}
const _: () = assert!(size_of::<InterruptContext>() == 5 * 8);

#[allow(dead_code)]
#[repr(C)]
#[derive(Clone, Copy)]
struct InterruptInfo {
    fpu_context: FPUContext,      // FPUの状態
    _dummy: u64,                  // アラインメント用のダミー
    greg: GeneralRegisterContext, // 汎用レジスタの状態
    error_code: u64,              // エラーコード
    ctx: InterruptContext,        // 割り込み時のコンテキスト
}
const _: () = assert!(size_of::<InterruptInfo>() == (16 + 4 + 1) * 8 + 8 + 512);

impl fmt::Debug for InterruptInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f,
        "
        {{
            rip: {:#018X}, cs: {:#06X},
            rsp: {:#018X}, ss: {:#06X},
            rbp: {:#018X}
            
            rflags: {:#018X},
            error_code: {:#018X}

            rax: {:#018X}, rcx: {:#018X}, 
            rdx: {:#018X}, rbx: {:#018X},
            rsi: {:#018X}, rdi: {:#018X},
            r8 : {:#018X}, r9 : {:#018X},
            r10: {:#018x}, r11: {:#018X},
            r12: {:#018X}, r13: {:#018X},
            r14: {:#018X}, r15: {:#018X},
        }}",
        self.ctx.rip, 
        self.ctx.cs,
        self.ctx.rsp, 
        self.ctx.ss,
        self.greg.rbp, 
        self.ctx.rflags,
        self.error_code,
        //
        self.greg.rax,
        self.greg.rcx,
        self.greg.rdx,
        self.greg.rbx,
        //
        self.greg.rsi,
        self.greg.rdi,
        //
        self.greg.r8,
        self.greg.r9,
        self.greg.r10,
        self.greg.r11,
        self.greg.r12,
        self.greg.r13,
        self.greg.r14,
        self.greg.r15
        )
    }
}

/// 割り込み番号ごとのエントリポイントを生成するマクロ
/// $index: 割り込み番号
/// x86_64の割り込みABIに沿ってエラーコード・割り込み番号を添えて共通ハンドラにジャンプ
macro_rules! interrupt_entrypoint {
    ($index:literal) => {
        global_asm!(concat!(
            ".global interrupt_entrypoint",
            stringify!($index),
            "\n",
            "interrupt_entrypoint",
            stringify!($index),
            ":\n",
            "push 0 \n", // エラーコードがない場合は0をプッシュ
            "push rcx \n",
            "mov rcx, ",
            stringify!($index),
            "\n",
            "jmp inthandler_common"
        ));
    }
}

macro_rules! interrupt_entrypoint_with_ecode {
    ($index:literal) => {
        global_asm!(concat!(
                ".global interrupt_entrypoint",
                stringify!($index),
                "\n",
                "interrupt_entrypoint",
                stringify!($index),
                ":\n",
                "push rcx\n",
                "mov rcx, ",
                stringify!($index),
                "\n",
                "jmp inthandler_common"
        )); 
    };
}

interrupt_entrypoint!(3);
interrupt_entrypoint!(6);
interrupt_entrypoint_with_ecode!(8);
interrupt_entrypoint_with_ecode!(13);
interrupt_entrypoint_with_ecode!(14);
interrupt_entrypoint!(32);

extern "sysv64" {
    fn interrupt_entrypoint3();
    fn interrupt_entrypoint6();
    fn interrupt_entrypoint8();
    fn interrupt_entrypoint13();
    fn interrupt_entrypoint14();
    fn interrupt_entrypoint32();
}

global_asm!(
    r#"
.global inthandler_common
    // 汎用レジスタの値を退避
    push r15
    push r14
    push r13
    push r12
    push r11
    push r10
    push r9
    push r8
    push rdi
    push rsi
    push rbp
    push rbx
    push rdx
    push rax
    // FPUの状態を退避(浮動小数点演算ユニット)
    sub rsp, 512 + 8
    fxsave64[rsp]
    // 関数に渡すパラメータの準備
    // | 引数    | レジスタ    |
    // | ----    | -------     |
    // | 第1引数 | **RDI**     |
    // | 第2引数 | RSI         |
    // | 第3引数 | RDX         |
    // | 第4引数 | RCX         |
    // | 第5引数 | R8          |
    // | 第6引数 | R9          |

    // 第一引数(現在のスタックフレームの全体を指すポインタ)
    mov rdi, rsp
    // 元のスタックの位置を保存
    mov rbp, rsp
    // ABI要求の16byteアラインメントを維持するために調整
    and rsp, -16
    // 第二引数(割り込み番号)
    mov rsi, rcx
    
    call inthander

    // 退避していた値を復元
    mov rsp, rbp
    fxrstor64[rsp]
    add rsp, 512 + 8
    pop rax
    pop rdx
    pop rbx
    pop rbp
    pop rsi
    pop rdi
    pop r8
    pop r9
    pop r10
    pop r11
    pop r12
    pop r13
    pop r14
    pop r15

    // 復帰処理
    pop rcx
    add rsp, 8
    iretq
    "#
);

/// CR2 レジスタの値を取得する関数
/// CR2 レジスタは、ページフォルトが発生した際の仮想アドレスを保持します。
/// つまりアクセスしようとして失敗した仮想アドレスを知るために使用される。
pub fn read_cr2() -> u64 {
    let cr2: u64;
    unsafe {
        asm!("mov rax, cr2",
        out("rax") cr2
        )
    }
    cr2
}

#[no_mangle]
extern  "sysv64" fn inthandler(info: &InterruptInfo, index: usize) {
    error!("Interrupt Info: {:?}", info);
    error!("Exception {index:#04X}: ");

    match index {
        3 => error!("Breakpoint Exception"),
        6 => error!("Invalid Opcode Exception"),
        8 => error!("Double Fault Exception"),
        13 =>{ error!("General Protection Fault");
            let rip = info.ctx.rip;
            error!("Bytes @ RIP({rip:#018X}): ");
            let rip = rip as *const u8;
            let bytes = unsafe { core::slice::from_raw_parts(rip, 16) };
            error!(" = {bytes:02X?}");
    },
        14 => {
            error!("Page Fault Exception");
            error!("CR2 = {:#018X}", read_cr2());
            error!("Caused by: A {} mode {} on a {} page, page structures are {}",
                if info.error_code & 0b0000_0100 != 0 {
                    "user"
                } else { 
                    "supervisor"
                },
                if info.error_code & 0b0001_0000 != 0 {
                    "instruction fetch"
                } else if info.error_code & 0b0000_0010 != 0 {
                    "data write"
                } else {
                    "data read"
                },
                if info.error_code & 0b0001 != 0 {
                    "present"
                } else {
                    "non-present"
                },
                if info.error_code & 0b1000 != 0{
                    "invalid"
                } else {
                    "valid"
                }
            );
        }
        _ => error!("Not handled"),
    }
    panic!("fatal exception");
}

#[no_mangle]
extern  "sysv64" fn int_handler_unimplemented() {
    panic!("unexpected interrupt");
}

// PDRTTTT (TTTT: Type, R: Reserved, D: DPL, P: Present)
pub const BIT_FLAGS_INTGATE: u8 = 0b0000_1110u8;
pub const BIT_FLAGS_PRESENT: u8 = 0b1000_0000u8;
// DPL: Descriptor Privilege Level(特権レベル)
pub const BIT_FLAGS_DPL0: u8 = 0 << 5;
pub const BIT_FLAGS_DPL3: u8 = 3 << 5;

#[repr(u8)]
#[derive(Clone, Copy)]
enum IdtAttr{
    _NotPresent = 0,
    IntGateDPL0 = BIT_FLAGS_INTGATE | BIT_FLAGS_DPL0 | BIT_FLAGS_PRESENT,
    IntGateDPL3 = BIT_FLAGS_INTGATE | BIT_FLAGS_DPL3 | BIT_FLAGS_PRESENT,
}

#[repr(C, packed)]
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct IdtDescriptor {
    offset_low: u16,
    segment_selector: u16,
    ist_index: u8,
    attr: IdtAttr,
    offset_mid: u16,
    offset_hight: u32,
    _reserved: u32,
}
const _ : () = assert!(size_of::<IdtDescriptor>() == 16);
impl IdtDescriptor {
    fn new(
        segment_selector: u16,
        ist_index: u8,
        attr: IdtAttr,
        f: unsafe extern "sysv64" fn(),
    ) -> Self {
        let handler_addr = f as *const unsafe extern "sysv64" fn() as usize;
        Self {
            offset_low: handler_addr as u16,
            offset_mid: (handler_addr >> 16) as u16,
            offset_hight: (handler_addr >> 32) as u32,
            segment_selector,
            ist_index,
            attr,
            _reserved: 0,
        }
    }
}

#[allow(dead_code)]
#[repr(C, packed)]
#[derive(Debug)]
struct IdtrParameters {
    limit: u16,
    base: *const IdtDescriptor,
}
const _: () = assert!(size_of::<IdtrParameters>() == 10);
const _: () = assert!(offset_of!(IdtrParameters, base) == 2);

pub struct Idt {
    #[allow(dead_code)]
    entries: Pin<Box<[IdtDescriptor; 0x100]>>,
} 
impl Idt {
   pub fn new(
    segment_selector: u16,
   )  -> Self {
        let mut entries = [IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0, // カーネルモード
            int_handler_unimplemented,
        ); 0x100];
        entries[3] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL3, // ユーザーモードからの割り込み許可
            interrupt_entrypoint3,
        );
        entries[6] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0, // カーネルモード
            interrupt_entrypoint6,
        );
        entries[8] = IdtDescriptor::new(
            segment_selector,
            2, // IST 2 を使用
            IdtAttr::IntGateDPL0, // カーネルモード
            interrupt_entrypoint8,
        );
        entries[13] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0, // カーネルモード
            interrupt_entrypoint13,
        );
        entries[14] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0, // カーネルモード
            interrupt_entrypoint14,
        );
        entries[32] = IdtDescriptor::new(
            segment_selector,
            1,
            IdtAttr::IntGateDPL0, // カーネルモード
            interrupt_entrypoint32,
        );
        let limit = size_of_val(&entries) as u16;
        let entries = Box::pin(entries);
        let params = IdtrParameters {
            limit,
            base: entries.as_ptr()
        };
        info!("Loading IDT: {params:?}");
        unsafe {
            asm!("lidt [rcx]", in("rcx") &params);
        };
        Self {
            entries 
        }
    }
}

#[repr(C, packed)]
struct TaskStateSegment64Inner {
    _reserved0: u32,
    _rsp: [u64; 3],
    _ist: [u64; 8],
    _reserved1: [u16; 5],
    _io_map_base_addr: u16,
}
const _: () = assert!(size_of::<TaskStateSegment64Inner>() == 104);

pub struct TaskStateSegment64 {
    inner: Pin<Box<TaskStateSegment64Inner>>,
}
impl TaskStateSegment64 {
    pub fn phys_addr(&self) -> u64 {
        self.inner.as_ref().get_ref() as *const TaskStateSegment64Inner as u64
    }
    unsafe fn alloc_interrupt_stack() -> u64 {
        const HANDLER_STACK_SIZE: usize = 64 * 1024;
        let stack = Box::new([0u8; HANDLER_STACK_SIZE]);
        // スタックの先頭アドレスを返す
        let rsp = unsafe {
            stack.as_ptr().add(HANDLER_STACK_SIZE) as u64
        };
        core::mem::forget(stack);
        rsp
    }
    pub fn new() -> Self {
        let rsp0 = unsafe { Self::alloc_interrupt_stack() };
        // 割り込みスタックテーブル
        let mut ist = [0u64; 8];
        for ist in ist[1..=7].iter_mut() {
            *ist = unsafe { Self::alloc_interrupt_stack() };
        }
        let tss64 = TaskStateSegment64Inner {
            _reserved0: 0,
            _rsp: [rsp0, 0, 0],
            _ist: ist,
            _reserved1: [0; 5],
            _io_map_base_addr: 0,
        };
        let this = Self {
            inner: Box::pin(tss64),
        };
        info!("TSS64 craeted @ {:#X}", this.phys_addr());
        this
    }
}
impl Drop for TaskStateSegment64 {
    fn drop(&mut self) {
        panic!("TSS64 being dropped!");
    }
}

pub fn init_exceptions() -> (GdtWrapper, Idt) {
    let gdt = GdtWrapper::default();
    gdt.load();
    unsafe {
        // コードセグメント(今どの特権レベルで実行されているかを示す)
        // mv cs, ... ←この操作はできない
        write_cs(KERNEL_CS);
        // スタックセグメント
        write_ss(KERNEL_DS);
        // 文字列命令用セグメント(歴史的経緯で互換性の為に存在)
        write_es(KERNEL_DS);
        // データセグメント(歴史的経緯で互換性の為に存在)
        write_ds(KERNEL_DS);
        // 汎用セグメント(TSLスレッドローカル, スレッド固有データへの高速アクセス)
        write_fs(KERNEL_DS);
        // 汎用セグメント(per-CPU/カーネル用)
        write_gs(KERNEL_DS);
    }
    let idt = Idt::new(KERNEL_CS);
    (gdt, idt)
}

pub const BIT_TYPE_DATA: u64 = 0b10u64 << 43;
pub const BIT_TYPE_CODE: u64 = 0b11u64 << 43;

pub const BIT_PRESENT: u64 = 1u64 << 47;
pub const BIT_CS_LONG_MODE: u64 = 1u64 << 53;
pub const BIT_CS_READABLE: u64 = 1u64 << 53;
pub const BIT_DS_WRITABLE: u64 = 1u64 << 41;
pub const BIT_DPL0: u64 = 0u64 << 45;
pub const BIT_DPL3: u64 = 3u64 << 45;

#[repr(u64)]
enum GdtAttr {
    KernelCode = 
        BIT_TYPE_CODE | BIT_PRESENT | BIT_CS_LONG_MODE | BIT_CS_READABLE ,
    KernelData = 
        BIT_TYPE_DATA | BIT_PRESENT | BIT_DS_WRITABLE,
}

#[allow(dead_code)]
#[repr(C, packed)]
struct GdtParameters {
    limit: u16,
    base: *const Gdt,
}

pub const KERNEL_CS : u16 = 1 << 3;
pub const KERNEL_DS : u16 = 2 << 3;
pub const TSS64_SEGMENT : u16 = 3 << 3;

#[allow(dead_code)]
#[repr(C, packed)]
pub struct Gdt {
    null_segment: GdtSegmentDescriptor,
    kernel_code_segment: GdtSegmentDescriptor,
    kernel_data_segment: GdtSegmentDescriptor,
    task_state_segment: TaskStateSegment64Descriptor,
}
const _ :() = assert!(size_of::<Gdt>() == 40);

#[allow(dead_code)]
pub struct GdtWrapper {
    inner: Pin<Box<Gdt>>,
    tss64: TaskStateSegment64,
}

impl GdtWrapper {
    pub fn load(&self)
    {
        let params = GdtrPrameters {
            limit: (size_of::<Gdt>() - 1) as u16,
            base: self.inner.as_ref().get_ref() as *const Gdt,
        };
        info!("Loading GDT @ {params:#018X}");
        unsafe {
            // 命令名：LGDT (Load Global Descriptor Table Register)
            // 役割：GDTR（GDT レジスタ）に GDT の情報をロード
            // いつ使う？：ブート時 / カーネル初期化時
            // 権限：特権命令（ring 0）
            // 
            // 汎用レジスタ RCX に GDT の情報を指すポインタをセットし、
            // LGDT 命令で GDTR にロードする。
            // csxにはGdtrPrameters構造体のアドレスが入っている
            asm!("lgdt [rcx]", in("rcx") &params);

        };
    }
}
impl Default for GdtWrapper {
    fn default() -> Self {
        let tss64 = TaskStateSegment64::new();
        let gdt = Gdt {
            null_segment: GdtSegmentDescriptor::null(),
            kernel_code_segment: GdtSegmentDescriptor::new(GdtAttr::KernelCode),
            kernel_data_segment: GdtSegmentDescriptor::new(GdtAttr::KernelData),
            task_state_segment: TaskStateSegment64Descriptor::new(tss64.phys_addr()),
        };
        Self {
            inner: Box::pin(gdt),
            tss64,
        }
    }
}

pub struct GdtSegmentDescriptor {
    value: u64,
}
impl GdtSegmentDescriptor {
    fn null() -> Self {
        Self { value: 0 }
    }
    fn new(attr: GdtAttr) -> Self {
        Self { value: attr as u64 }
    }
}   
impl Display for GdtSegmentDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:#018X}", self.value)
    }
}

#[repr(C, packed)]
#[allow(dead_code)]
struct TaskStateSegment64Descriptor {
    limit_low: u16,
    base_low: u16,
    base_mid_low: u8,
    attr: u16,
    base_mid_hight: u32,
    base_high: u32,
    reserved: u32,
}
impl TaskStateSegment64Descriptor {
    fn new(base_addr: u64) -> Self {
        Self {
            limit_low: size_of::<TaskStateSegment64Inner>() as u16,
            base_low: (base_addr & 0xffff) as u16,
            base_mid_low: ((base_addr >> 16) & 0xff) as u8,
            attr: 0b1000_0000_1000_0001, 
            base_mid_hight: ((base_addr >> 24) & 0xff) as u32,
            base_high: ((base_addr >> 32) & 0xffffffff) as u32,
            reserved: 0,
        }
    }
}
const _: () = assert!(size_of::<TaskStateSegment64Descriptor>() == 16);

pub fn trigger_debug_interrupt() {
    unsafe {
        asm!("int3");
    }
}