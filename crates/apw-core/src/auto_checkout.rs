//! Apple Store 自动下单与极速抢单脚本生成与调度支持。
//!
//! 在库存监控命中时，通过自动化脚本在真实浏览器（拥有完整 Apple ID 登录态与
//! Akamai 安全 Cookie）中执行秒级操作：
//! 1. 自动处理 Apple 官网选配页的前置必选项（跳过换购估价、全款结账、跳过 AppleCare+）；
//! 2. 激活并点击加购按钮入袋；
//! 3. 购物袋快速点击结账；
//! 4. 智能匹配并锁定监控的目标零售店（按门店编号如 R683 或名称模糊匹配）；
//! 5. 毫秒级抢占最早可用的预约到店取货时间段（Time Slot）；
//! 6. 自动填写取货人姓名、身份证号、手机号与邮箱（通过 React Synthetic Event 触发）；
//! 7. 推进至支付页面，选中首选支付方式（支付宝/微信），弹出付款二维码等待扫码。

use crate::config::AutoCheckoutConfig;
use crate::model::Target;

/// 生成专为当前配置定制的 Tampermonkey 油猴抢购脚本。
pub fn generate_userscript(config: &AutoCheckoutConfig, targets: &[Target]) -> String {
    let full_name = serde_json::to_string(&config.full_name).unwrap_or_else(|_| "\"\"".into());
    let id_card = serde_json::to_string(&config.id_card_number).unwrap_or_else(|_| "\"\"".into());
    let phone = serde_json::to_string(&config.phone_number).unwrap_or_else(|_| "\"\"".into());
    let email = serde_json::to_string(&config.email).unwrap_or_else(|_| "\"\"".into());
    let payment_method = serde_json::to_string(&config.payment_method).unwrap_or_else(|_| "\"alipay\"".into());
    let time_pref = serde_json::to_string(&config.time_slot_preference).unwrap_or_else(|_| "\"earliest\"".into());

    let mut store_list = Vec::new();
    let mut seen_stores = std::collections::HashSet::new();
    for t in targets {
        if seen_stores.insert(t.store_number.clone()) {
            store_list.push(serde_json::json!({
                "number": t.store_number,
                "title": t.store_title,
            }));
        }
    }
    let target_stores_json = serde_json::to_string(&store_list).unwrap_or_else(|_| "[]".into());

    let target_parts: Vec<&str> = targets.iter().map(|t| t.part_number.as_str()).collect();
    let target_parts_json = serde_json::to_string(&target_parts).unwrap_or_else(|_| "[]".into());

    format!(
r#"// ==UserScript==
// @name         Apple Store 极速自动抢单与结账助手（果到雷达专享版）
// @namespace    https://github.com/samzhang996-hue/apple-store-inventory-monitor
// @version      1.1.0
// @description  到店取货库存命中后，毫秒级自动跳过折抵与AppleCare、加购、锁定门店、抢占预约时段、填充身份信息并直达支付二维码页面。
// @author       果到雷达 (Apple Store Inventory Monitor)
// @match        https://www.apple.com.cn/shop/*
// @match        https://www.apple.com/hk-zh/shop/*
// @match        https://www.apple.com/tw/shop/*
// @match        https://www.apple.com/jp/shop/*
// @run-at       document-idle
// @grant        none
// ==/UserScript==

(function() {{
    'use strict';

    // 由「果到雷达」自动同步的抢单配置
    const CONFIG = {{
        enabled: {enabled},
        fullName: {full_name},
        idCardNumber: {id_card},
        phoneNumber: {phone},
        email: {email},
        paymentMethod: {payment_method}, // alipay, wechat, none
        timeSlotPreference: {time_pref}, // earliest, any_today
        targetStores: {target_stores_json},
        targetParts: {target_parts_json}
    }};

    if (!CONFIG.enabled) {{
        console.log('[果到雷达] 自动下单助手未开启或已停用');
        return;
    }}

    console.log('[果到雷达] 极速抢单助手已就绪，正在监听页面状态...', CONFIG);

    // 状态机步骤标记，避免重复点击
    const state = {{
        selectedTradeIn: false,
        selectedAppleCare: false,
        clickedAdd: false,
        clickedCheckout: false,
        selectedPickup: false,
        selectedStore: false,
        selectedTimeSlot: false,
        filledIdentity: false,
        selectedPayment: false,
        completed: false
    }};

    // 页面悬浮状态 HUD
    function showHUD(text, isDone = false) {{
        let hud = document.getElementById('apw-checkout-hud');
        if (!hud) {{
            hud = document.createElement('div');
            hud.id = 'apw-checkout-hud';
            hud.style.position = 'fixed';
            hud.style.top = '18px';
            hud.style.right = '18px';
            hud.style.zIndex = '999999';
            hud.style.padding = '12px 18px';
            hud.style.borderRadius = '10px';
            hud.style.fontFamily = '-apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif';
            hud.style.fontSize = '14px';
            hud.style.fontWeight = '600';
            hud.style.boxShadow = '0 8px 30px rgba(0,0,0,0.25)';
            hud.style.transition = 'all 0.3s ease';
            document.body.appendChild(hud);
        }}
        hud.style.background = isDone ? '#10b981' : '#2563eb';
        hud.style.color = '#ffffff';
        hud.innerHTML = `⚡ 果到雷达抢单助手：${{text}}`;
    }}

    // 播放提示蜂鸣音
    function playBeep() {{
        try {{
            const ctx = new (window.AudioContext || window.webkitAudioContext)();
            const osc = ctx.createOscillator();
            const gain = ctx.createGain();
            osc.connect(gain);
            gain.connect(ctx.destination);
            osc.frequency.value = 880;
            osc.type = 'sine';
            gain.gain.value = 0.2;
            osc.start();
            setTimeout(() => {{
                osc.stop();
                ctx.close();
            }}, 200);
        }} catch(e) {{}}
    }}

    // React 19 / SyntheticEvent 安全触发输入
    function setReactInput(input, val) {{
        if (!input || !val) return;
        const nativeSetter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')?.set;
        if (nativeSetter) {{
            nativeSetter.call(input, val);
        }} else {{
            input.value = val;
        }}
        input.dispatchEvent(new Event('input', {{ bubbles: true }}));
        input.dispatchEvent(new Event('change', {{ bubbles: true }}));
        input.dispatchEvent(new Event('blur', {{ bubbles: true }}));
    }}

    // 辅助：智能模糊匹配零售店
    function matchStore(card, store) {{
        if (!card || !store) return false;
        const text = card.textContent || '';
        const html = card.innerHTML || '';
        const num = (store.number || '').trim();
        if (num && (html.includes(num) || text.includes(num))) {{
            return true;
        }}
        const title = (store.title || '').trim();
        if (!title) return false;
        if (text.includes(title)) return true;
        const cleanTitle = title.includes('-') ? title.split('-').slice(1).join('-').trim() : title;
        if (cleanTitle && text.includes(cleanTitle)) return true;
        const subTitle = cleanTitle.replace(/^(北京|上海|天津|重庆|四川|广东|江苏|浙江|山东|湖北|辽宁|陕西|河南|福建|成都|武汉|杭州|南京|广州|深圳|沈阳|大连|无锡|苏州|宁波|厦门|福州|青岛|济南|郑州|长沙|南宁|西安|昆明|贵阳)/, '');
        if (subTitle && subTitle.length >= 2 && text.includes(subTitle)) return true;
        return false;
    }}

    // 步骤 1：商品选配与详情页快速推进（自动跳过折抵与 AppleCare+，秒级加购）
    function handleProductPage() {{
        // 1.1 加购后弹出的侧边抽屉、配件选配页（step=attach）或浮层，优先点击「结账」或「查看购物袋」
        const overlayProceedBtn = document.querySelector(
            'button[name="proceed"], button[data-autom="proceed"], button[data-autom="reviewBag"], button[data-autom="proceed-to-checkout"], button[data-autom="checkout"], a[data-autom="checkout"], a[data-autom="proceed"], button.as-overlay-action, a[href*="/shop/bag"]'
        );
        if (overlayProceedBtn && !overlayProceedBtn.disabled) {{
            showHUD('商品已入袋，正在极速进入购物袋/结账...', false);
            overlayProceedBtn.click();
            return;
        }}

        // 若直接进入了配件推荐页 (step=attach)，且存在查看购物袋按钮，直接点击推进
        if (window.location.search.includes('step=attach')) {{
            const attachProceedBtn = document.querySelector('button[data-autom="proceed"], button[name="proceed"], a[href*="/shop/bag"]');
            if (attachProceedBtn && !attachProceedBtn.disabled) {{
                showHUD('已跳过配件推荐，正在进入购物袋...');
                attachProceedBtn.click();
                return;
            }}
        }}

        // 1.2 自动选择「不折抵换购」(Apple Trade In)
        if (!state.selectedTradeIn) {{
            const tradeInRadio = document.querySelector(
                'input[data-autom="choose-noTradeIn"], input#noTradeIn, input[value="noTradeIn"], input[name*="tradeup"][value="false"]'
            );
            if (tradeInRadio) {{
                if (!tradeInRadio.checked) {{
                    showHUD('正在自动选择「不折抵换购」...');
                    const label = document.querySelector('label[id="noTradeIn_label"], label[for="noTradeIn"]') || tradeInRadio.closest('label') || tradeInRadio;
                    label.click();
                    tradeInRadio.click();
                }}
                state.selectedTradeIn = true;
            }} else {{
                const labels = document.querySelectorAll('label, button, [role="radio"]');
                for (const el of labels) {{
                    const text = (el.textContent || '').trim();
                    if ((text.includes('不折抵换购') || text.includes('无折抵换购') || text.includes('不進行折抵換購') || text.includes('没有要换购')) && !el.classList.contains('as-selected')) {{
                        el.click();
                        state.selectedTradeIn = true;
                        showHUD('已选择「不折抵换购」');
                        break;
                    }}
                }}
            }}
        }}

        // 1.3 自动选择「全款 / 一次性付清」（若页面要求选择购买方式）
        const fullPriceRadio = document.querySelector('input[data-autom*="fullprice"], input[value*="fullprice"], input[value*="full-price"]');
        if (fullPriceRadio && !fullPriceRadio.checked) {{
            const label = document.querySelector(`label[for="${{fullPriceRadio.id}}"]`) || fullPriceRadio.closest('label') || fullPriceRadio;
            label.click();
            fullPriceRadio.click();
        }}

        // 1.4 自动跳过「AppleCare+ 服务计划」
        if (!state.selectedAppleCare) {{
            const noAppleCareRadio = document.querySelector(
                'input[data-autom="noapplecare"], input#noapplecare, input[value="noapplecare"], input[name*="applecare"][value*="no"]'
            );
            if (noAppleCareRadio) {{
                if (!noAppleCareRadio.checked) {{
                    showHUD('正在自动跳过 AppleCare+...');
                    const acLabel = document.querySelector(`label[for="${{noAppleCareRadio.id}}"], label[for="noapplecare"], .rf-applecare-label`) || noAppleCareRadio.closest('label') || noAppleCareRadio;
                    acLabel.click();
                    noAppleCareRadio.click();
                }}
                state.selectedAppleCare = true;
            }} else {{
                const labels = document.querySelectorAll('label, button, [role="radio"]');
                for (const el of labels) {{
                    const text = (el.textContent || '').trim();
                    if ((text.includes('不加 AppleCare+') || text.includes('不需要 AppleCare+') || text.includes('不添加 AppleCare+')) && !el.classList.contains('as-selected')) {{
                        el.click();
                        state.selectedAppleCare = true;
                        showHUD('已跳过 AppleCare+');
                        break;
                    }}
                }}
            }}
        }}

        // 1.5 点击「添加到购物袋」主加购按钮
        if (!state.clickedAdd) {{
            const addBtn = document.querySelector(
                'button[name="add-to-cart"], button[data-autom="add-to-cart"], button[data-autom="addToCart"], button[id*="add-to-cart"], button[type="submit"].as-purchaseinfo-button, button.as-purchaseinfo-button, .as-buyflow-addtocart button'
            );
            if (addBtn && !addBtn.disabled) {{
                state.clickedAdd = true;
                showHUD('选配就绪，正在极速加入购物袋...');
                addBtn.click();
                playBeep();
                // 如果加购后未自动跳转，1.2 秒后保底直跳购物袋推进
                setTimeout(() => {{
                    if (window.location.href.includes('/shop/buy-') || window.location.href.includes('/shop/product/')) {{
                        const currentBagBtn = document.querySelector('button[name="proceed"], button[data-autom="proceed-to-checkout"], a[href*="/shop/bag"]');
                        if (currentBagBtn) {{
                            currentBagBtn.click();
                        }} else {{
                            window.location.href = 'https://www.apple.com.cn/shop/bag';
                        }}
                    }}
                }}, 1200);
            }}
        }}
    }}

    // 步骤 2：购物袋页面点击结账
    function handleBagPage() {{
        if (state.clickedCheckout) return;
        const checkoutBtn = document.querySelector(
            '#shoppingCart\\.actions\\.navCheckout, #shoppingCart\\.actions\\.checkout, button[name="proceed"], button[data-autom="checkout"], button.as-bag-action, button[data-autom="bag-checkout-button"]'
        );
        if (checkoutBtn && !checkoutBtn.disabled) {{
            state.clickedCheckout = true;
            showHUD('正在自动点击结账...', false);
            playBeep();
            setTimeout(() => checkoutBtn.click(), 50);
        }} else {{
            const emptyNotice = document.querySelector('.as-shoppingcart-empty, [data-autom*="empty"]');
            if (emptyNotice) {{
                showHUD('⚠️ 购物袋当前为空，正在等待商品入袋...', false);
            }}
        }}
    }}

    // 步骤 3：访客结账确认（若未登录）
    function handleSignInPage() {{
        let guestBtn = document.querySelector(
            'button[id*="guestLogin"], button#signIn\\.guestLogin\\.guestLogin, button[id="guest-checkout"], button[data-autom="guest-checkout"], button[data-autom*="guest"]'
        );
        if (!guestBtn) {{
            const buttons = document.querySelectorAll('button, a[role="button"]');
            for (const b of buttons) {{
                const t = (b.textContent || '').trim();
                if (t.includes('以游客身份继续') || t.includes('访客结账') || t.includes('游客结账') || t.includes('以訪客身分繼續') || t.includes('Continue as Guest')) {{
                    guestBtn = b;
                    break;
                }}
            }}
        }}
        if (guestBtn && !guestBtn.disabled) {{
            showHUD('检测到游客结账，正在自动进入...');
            guestBtn.click();
        }} else {{
            showHUD('请登录 Apple ID 以继续抢单');
        }}
    }}

    // 步骤 4：履约方式选择（选择「到店自提」与智能匹配门店）
    function handleFulfillment() {{
        // 选择零售店取货
        if (!state.selectedPickup) {{
            const pickupRadio = document.querySelector('input[type="radio"][value="pickup"], input[type="radio"][id*="pickup"], button[data-autom*="pickup"]');
            if (pickupRadio) {{
                pickupRadio.click();
                state.selectedPickup = true;
                showHUD('已自动选择「零售店自提」');
            }}
        }}

        // 匹配目标零售店
        if (!state.selectedStore) {{
            const storeCards = document.querySelectorAll('[data-autom*="store-list"] label, .as-retail-store, [data-autom*="store-item"], .as-storelocator-storeitem');
            if (storeCards.length > 0) {{
                let matchedCard = null;
                let matchedName = '';

                // 优先在用户的监控门店列表中寻找匹配
                if (CONFIG.targetStores && CONFIG.targetStores.length > 0) {{
                    for (const store of CONFIG.targetStores) {{
                        for (const card of storeCards) {{
                            if (matchStore(card, store)) {{
                                matchedCard = card;
                                matchedName = store.title || store.number;
                                break;
                            }}
                        }}
                        if (matchedCard) break;
                    }}
                }}

                // 如果未精准匹配到监控门店，但列表只有一个门店，回退为列表中第一个门店
                if (!matchedCard && storeCards.length === 1) {{
                    matchedCard = storeCards[0];
                    matchedName = matchedCard.textContent.trim().split('\n')[0];
                }}

                if (matchedCard) {{
                    const radio = matchedCard.querySelector('input[type="radio"]') || matchedCard;
                    radio.click();
                    state.selectedStore = true;
                    showHUD(`已锁定自提门店：${{matchedName}}`);
                }}
            }}
        }}

        // 推进取货选择
        const continueBtn = document.querySelector('button[data-autom="checkout-fulfillment-continue-button"], button[name="continue"]');
        if (continueBtn && !continueBtn.disabled) {{
            continueBtn.click();
        }}
    }}

    // 步骤 5：秒抢预约时段 (Time Slot)
    function handleTimeSlot() {{
        if (state.selectedTimeSlot) return;
        const timeSlotRadios = Array.from(document.querySelectorAll(
            'input[name*="timeSlot"]:not(:disabled), input[id*="timeSlot"]:not(:disabled), [data-autom*="time-slot"] input:not(:disabled)'
        ));

        if (timeSlotRadios.length > 0) {{
            // 优先抢占首个可用时段
            const targetSlot = timeSlotRadios[0];
            targetSlot.click();
            targetSlot.dispatchEvent(new Event('change', {{ bubbles: true }}));
            state.selectedTimeSlot = true;
            playBeep();
            showHUD('🎉 已秒抢可用预约时段！正在推进...', false);

            setTimeout(() => {{
                const nextBtn = document.querySelector('button[data-autom="continue"], button[name="continue"]');
                if (nextBtn && !nextBtn.disabled) nextBtn.click();
            }}, 80);
        }}
    }}

    // 步骤 6：自动填写真实身份核验信息
    function handleIdentity() {{
        if (state.filledIdentity) return;

        let hasFilled = false;
        // 姓名
        const nameInput = document.querySelector('input[name*="firstName"], input[name*="fullName"], input[id*="firstName"], input[data-autom*="firstName"]');
        if (nameInput && !nameInput.value && CONFIG.fullName) {{
            setReactInput(nameInput, CONFIG.fullName);
            hasFilled = true;
        }}

        // 身份证后 4 位 (大陆自提实名核验)
        const idInput = document.querySelector('input[name*="nationalId"], input[name*="idNumber"], input[id*="idNumber"], input[data-autom*="idNumber"], input[name*="last4"], input[placeholder*="身份证"]');
        if (idInput && !idInput.value && CONFIG.idCardNumber) {{
            setReactInput(idInput, CONFIG.idCardNumber);
            hasFilled = true;
        }}

        // 手机号码
        const phoneInput = document.querySelector('input[name*="daytimePhone"], input[name*="phone"], input[id*="phone"], input[data-autom*="daytimePhone"]');
        if (phoneInput && !phoneInput.value && CONFIG.phoneNumber) {{
            setReactInput(phoneInput, CONFIG.phoneNumber);
            hasFilled = true;
        }}

        // 邮箱
        const emailInput = document.querySelector('input[name*="emailAddress"], input[name*="email"], input[id*="email"], input[data-autom*="emailAddress"]');
        if (emailInput && !emailInput.value && CONFIG.email) {{
            setReactInput(emailInput, CONFIG.email);
            hasFilled = true;
        }}

        if (hasFilled) {{
            state.filledIdentity = true;
            showHUD('已填妥取货人身份证与联系信息，正在前往付款...');
            setTimeout(() => {{
                const nextBtn = document.querySelector('button[data-autom="continue"], button[name="continue"]');
                if (nextBtn && !nextBtn.disabled) nextBtn.click();
            }}, 100);
        }}
    }}

    // 步骤 7：支付方式匹配与到达最终付款页
    function handlePayment() {{
        if (state.selectedPayment) return;

        // 首选支付方式切换
        if (CONFIG.paymentMethod === 'alipay') {{
            const alipayRadio = document.querySelector('input[value*="alipay"], input[id*="alipay"], label[for*="alipay"]');
            if (alipayRadio) alipayRadio.click();
        }} else if (CONFIG.paymentMethod === 'wechat') {{
            const wechatRadio = document.querySelector('input[value*="wechat"], input[id*="wechat"], label[for*="wechat"]');
            if (wechatRadio) wechatRadio.click();
        }}

        // 检查是否到达最终付款或展示二维码
        const isPaymentPage = window.location.href.includes('payment') || window.location.href.includes('billing') || document.querySelector('.as-payment-form, [data-autom*="payment"]');
        if (isPaymentPage) {{
            state.selectedPayment = true;
            state.completed = true;
            playBeep();
            showHUD('🎉 抢单全流程就绪！已到达付款阶段，请打开手机扫码完成支付！', true);
        }}
    }}

    // 主事件循环驱动
    function tick() {{
        const url = window.location.href;
        if (url.includes('/shop/buy-') || url.includes('/shop/product/')) {{
            handleProductPage();
        }} else if (url.includes('/shop/bag')) {{
            handleBagPage();
        }} else if (url.includes('/shop/signIn')) {{
            handleSignInPage();
        }} else if (url.includes('/shop/checkout')) {{
            handleFulfillment();
            handleTimeSlot();
            handleIdentity();
            handlePayment();
        }}
    }}

    // 快速轮询 + MutationObserver 双保险监听
    const observer = new MutationObserver(() => tick());
    observer.observe(document.documentElement, {{ childList: true, subtree: true }});
    setInterval(tick, 300);
    tick();
}})();
"#,
        enabled = config.enabled,
        full_name = full_name,
        id_card = id_card,
        phone = phone,
        email = email,
        payment_method = payment_method,
        time_pref = time_pref,
        target_stores_json = target_stores_json,
        target_parts_json = target_parts_json,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 生成脚本包含用户配置与防转义() {
        let config = AutoCheckoutConfig {
            enabled: true,
            full_name: "张三".into(),
            id_card_number: "1234".into(),
            phone_number: "13800138000".into(),
            email: "test@example.com".into(),
            time_slot_preference: "earliest".into(),
            payment_method: "alipay".into(),
        };
        let targets = vec![Target {
            locale: "zh_CN".into(),
            store_number: "R683".into(),
            store_title: "上海-环球港".into(),
            part_number: "MG6X4CH/A".into(),
            product_name: "iPhone 17 Pro 256GB".into(),
            companion_part: None,
            companion_name: None,
            kit_part: None,
        }];

        let script = generate_userscript(&config, &targets);
        assert!(script.contains("// ==UserScript=="));
        assert!(script.contains("fullName: \"张三\""));
        assert!(script.contains("idCardNumber: \"1234\""));
        assert!(script.contains("R683"));
        assert!(script.contains("上海-环球港"));
        assert!(script.contains("choose-noTradeIn"));
        assert!(script.contains("noapplecare"));
        assert!(script.contains("enabled: true"));
    }

    #[test]
    fn 禁用状态下脚本保持标记() {
        let config = AutoCheckoutConfig::default();
        let script = generate_userscript(&config, &[]);
        assert!(script.contains("enabled: false"));
    }
}
