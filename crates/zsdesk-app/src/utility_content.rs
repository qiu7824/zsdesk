use super::*;

pub(super) struct InfoRow{pub section:bool,pub key:String,pub description:String}
pub(super) fn shortcut_rows(body:&str)->Vec<InfoRow>{
    let mut rows=Vec::new();
    for line in body.lines().map(str::trim).filter(|line|!line.is_empty()){
        if ["文件管理器","桌面格子","全局搜索"].iter().any(|title|line==*title||line==zsdesk_core::i18n::text(title)){
            rows.push(InfoRow{section:true,key:String::new(),description:line.into()});continue;
        }
        for part in line.split('　').map(str::trim).filter(|part|!part.is_empty()){
            let chinese=part.char_indices().find(|(_,c)|('\u{3400}'..='\u{9fff}').contains(c)).map(|(index,_)|index);
            let pair=match chinese{Some(index)if index>0=>Some((&part[..index],&part[index..])),Some(0)=>part.split_once(' '),_=>part.split_once("  ")};
            let(key,description)=pair.unwrap_or(("",part));
            rows.push(InfoRow{section:false,key:key.trim().into(),description:description.trim().into()});
        }
    }rows
}
pub(super) fn rounded(dc:HDC,rect:RECT,fill:COLORREF,border:COLORREF,radius:i32){
    let brush=unsafe{CreateSolidBrush(fill)};let pen=unsafe{CreatePen(PS_SOLID,1,border)};
    let old_brush=unsafe{SelectObject(dc,brush.into())};let old_pen=unsafe{SelectObject(dc,pen.into())};
    unsafe{let _=RoundRect(dc,rect.left,rect.top,rect.right,rect.bottom,radius,radius);SelectObject(dc,old_pen);SelectObject(dc,old_brush);
        let _=DeleteObject(pen.into());let _=DeleteObject(brush.into());}
}
fn wrapped(dc:HDC,text:&str,rect:RECT,font:HFONT,color:COLORREF){
    let old=unsafe{SelectObject(dc,font.into())};let mut area=rect;let mut text:Vec<_>=text.encode_utf16().collect();
    unsafe{SetBkMode(dc,TRANSPARENT);SetTextColor(dc,color);DrawTextW(dc,&mut text,&mut area,DT_WORDBREAK|DT_NOPREFIX|DT_END_ELLIPSIS);SelectObject(dc,old);}
}
fn text_extent(dc:HDC,font:HFONT,value:&str,width:i32,flags:DRAW_TEXT_FORMAT)->RECT{
    let mut rect=RECT{right:width.max(1),..Default::default()};
    let mut text:Vec<_>=value.encode_utf16().collect();
    let previous=unsafe{SelectObject(dc,font.into())};
    unsafe{DrawTextW(dc,&mut text,&mut rect,flags|DT_NOPREFIX|DT_CALCRECT);SelectObject(dc,previous);}
    rect
}

fn about_action_rows(left:i32,right:i32,gap:i32,widths:[i32;3])->([(i32,i32);3],i32){
    let mut positions=[(0,0);3];
    let (mut x,mut row)=(left,0);
    for (i,w) in widths.into_iter().enumerate() {
        if x>left && x+w>right {row+=1;x=left;}
        positions[i]=(if i==2{right-w}else{x},row);
        x+=w+gap;
    }
    (positions,row)
}

struct TextBlock<'a>{text:&'a str,rect:RECT,font:HFONT,color:COLORREF}

impl Panel{
    fn about_blocks<'a>(&'a self,dc:HDC,width:i32,version:&'a str)->(Vec<TextBlock<'a>>,i32){
        let p=&self.palette;
        let primary=composite(p.text,p.layer);
        let secondary=composite(p.text_secondary,p.layer);
        let mut blocks=Vec::new();
        let mut y=self.s(4);
        let mut line=|value:&'a str,left:i32,top:i32,font:HFONT,color:COLORREF|{
            let h=text_extent(dc,font,value,width-left,DT_WORDBREAK).bottom;
            let bottom=top+h;
            blocks.push(TextBlock{text:value,rect:RECT{left,top,right:width,bottom},font,color});
            bottom
        };
        let indent=self.s(72);
        y=line("ZSDesk",indent,y,self.title_font,primary);
        y=line(version,indent,y+self.s(3),self.font,secondary);
        y=y.max(self.s(58))+self.s(18);
        let description=if crate::edition::DESKTOP_ONLY {"桌面格子"} else if crate::edition::DESKTOP {
            "文件浏览、快速搜索、内容预览与桌面格子。"
        }else{"文件浏览、快速搜索与内容预览。"};
        y=line(zsdesk_core::i18n::text(description),0,y,self.font,primary);
        if !self.status.is_empty(){
            let color=if self.search_error{rgb(p.danger)}else{secondary};
            y=line(&self.status,0,y+self.s(18),self.font,color);
        }
        (blocks,y+self.s(8))
    }

    pub(super) fn layout_about(&self,width:i32,height:i32){
        let margin=self.s(20);
        let gap=self.s(8);
        let right=(width-margin).max(margin+1);
        let link=unsafe{GetDlgItem(Some(self.hwnd),14)}.unwrap_or_default();
        let check=unsafe{GetDlgItem(Some(self.hwnd),15)}.unwrap_or_default();
        let dc=unsafe{GetDC(Some(self.hwnd))};
        let controls=[self.action,link,self.close];
        let widths=controls.map(|h| (text_extent(dc,self.font,&text(h),16384,DT_SINGLELINE).right+self.s(28)).max(self.s(76)).min(right-margin));
        let row_height=self.s(34);
        unsafe{ReleaseDC(Some(self.hwnd),dc);}
        let (positions,row)=about_action_rows(margin,right,gap,widths);
        let top=height-margin-row_height-row*(row_height+gap);
        let place=|h:HWND,x:i32,y:i32,w:i32,hgt:i32| if !h.is_invalid(){unsafe{
            let _=SetWindowPos(h,None,x,y,w.max(1),hgt.max(1),SWP_NOZORDER|SWP_NOACTIVATE);
        }};
        for (i,h) in controls.into_iter().enumerate(){
            place(h,positions[i].0,top+positions[i].1*(row_height+gap),widths[i],row_height);
        }
        let check_h=self.s(26);
        let check_y=top-gap-check_h;
        place(check,margin,check_y,right-margin,check_h);
        place(self.edit,margin,margin,right-margin,check_y-2*margin);
    }
    fn row_height(&self,row:&InfoRow)->i32{self.s(if row.section{38}else if row.key.is_empty(){62}else{48})}
    fn content_height(&self)->i32{
        if self.kind==Kind::About{
            let mut r=RECT::default();let _=unsafe{GetClientRect(self.edit,&mut r)};
            let dc=unsafe{GetDC(Some(self.edit))};
            let version=format!("{} · {}",env!("CARGO_PKG_VERSION"),crate::edition::name());
            let height=self.about_blocks(dc,r.right,&version).1;
            unsafe{ReleaseDC(Some(self.edit),dc);}return height;
        }
        self.info.iter().map(|row|self.row_height(row)).sum::<i32>()+self.s(12)
    }
    pub(super) fn update_scroll(&mut self){
        if self.kind==Kind::Search{return;}
        let mut rect=RECT::default();let _=unsafe{GetClientRect(self.edit,&mut rect)};
        let height=self.content_height();
        self.scroll=self.scroll.clamp(0,(height-rect.bottom).max(0));
        let info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as u32,fMask:SIF_RANGE|SIF_PAGE|SIF_POS,nMin:0,
            nMax:height.saturating_sub(1),nPage:rect.bottom.max(1)as u32,nPos:self.scroll,..Default::default()};
        unsafe{SetScrollInfo(self.edit,SB_VERT,&info,true);}
    }
    pub(super) fn scroll_info(&mut self,code:u32,wheel:i32){
        if self.kind==Kind::Search{return;}
        let mut rect=RECT::default();let _=unsafe{GetClientRect(self.edit,&mut rect)};
        let mut info=SCROLLINFO{cbSize:std::mem::size_of::<SCROLLINFO>()as u32,fMask:SIF_TRACKPOS,..Default::default()};
        let _=unsafe{GetScrollInfo(self.edit,SB_VERT,&mut info)};
        let max=(self.content_height()-rect.bottom).max(0);
        let next=if wheel!=0{self.scroll-wheel*self.s(120)/120}else{match code{
            0=>self.scroll-self.s(48),1=>self.scroll+self.s(48),2=>self.scroll-rect.bottom,3=>self.scroll+rect.bottom,
            4|5=>info.nTrackPos,6=>0,7=>max,_=>self.scroll,
        }};
        self.scroll=next.clamp(0,max);self.update_scroll();let _=unsafe{InvalidateRect(Some(self.edit),None,false)};
    }
    pub(super) fn paint_info(&self,dc:HDC,rect:RECT){
        unsafe{FillRect(dc,&rect,self.brush);}
        let p=&self.palette;let text=composite(p.text,p.layer);let secondary=composite(p.text_secondary,p.layer);
        if self.kind==Kind::About {
            let version=format!("{} · {}",env!("CARGO_PKG_VERSION"),crate::edition::name());
            if let Ok(module)=unsafe{GetModuleHandleW(None)}{
                if let Ok(icon)=unsafe{LoadIconW(Some(HINSTANCE(module.0)),PCWSTR(101usize as*const u16))}{
                    let _=unsafe{DrawIconEx(dc,0,self.s(4)-self.scroll,icon,self.s(54),self.s(54),0,None,DI_NORMAL)};
                }
            }
            for mut block in self.about_blocks(dc,rect.right,&version).0 {
                block.rect.top-=self.scroll;block.rect.bottom-=self.scroll;
                if block.rect.bottom>0 && block.rect.top<rect.bottom {
                    wrapped(dc,block.text,block.rect,block.font,block.color);
                }
            }
            return;
        }
        let saved=unsafe{SaveDC(dc)};unsafe{IntersectClipRect(dc,rect.left,rect.top,rect.right,rect.bottom);}
        let mut y=self.s(4)-self.scroll;
        for row in &self.info{
            let height=self.row_height(row);let bottom=y+height;
            if bottom>=0&&y<rect.bottom{
                if row.section{
                    draw_text(dc,&row.description,RECT{left:self.s(4),top:y,right:rect.right,bottom},self.font,secondary);
                }else{
                    let card=RECT{left:0,top:y,right:rect.right-self.s(4),bottom:bottom-self.s(3)};
                    rounded(dc,card,rgb(p.input),composite(p.divider,p.input),self.s(8));
                    let key_width=self.s(252).min((rect.right/2).max(self.s(120)));
                    let key_left=rect.right-key_width-self.s(16);
                    let description=RECT{left:self.s(16),top:y+self.s(10),right:if row.key.is_empty(){card.right-self.s(16)}else{key_left-self.s(14)},bottom:bottom-self.s(6)};
                    wrapped(dc,&row.description,description,self.font,composite(p.text,p.input));
                    if !row.key.is_empty(){
                        let key=RECT{left:key_left,top:y+self.s(8),right:card.right-self.s(12),bottom:bottom-self.s(10)};
                        rounded(dc,key,composite(p.hover,p.input),composite(p.divider,p.input),self.s(6));
                        draw_text(dc,&row.key,RECT{left:key.left+self.s(10),right:key.right-self.s(6),..key},self.font,composite(p.text,p.input));
                    }
                }
            }y=bottom;
        }
        if saved!=0{unsafe{RestoreDC(dc,saved);}}
    }
}

#[cfg(test)]mod tests{
    use super::*;
    #[test]
    fn about_footer_wraps_and_aligns_actions_at_each_scale(){
        for dpi in [96,120,144,192,288] {
            let s=|v| zsdesk_ui::chrome::scale(v,dpi);
            for available in [270,340,480,700] {
                let widths=[s(145),s(136),s(80)];
                let left=s(20);let right=s(available)-left;
                let (positions,last)=about_action_rows(left,right,s(8),widths);
                for i in 0..3 {
                    let (x,row)=positions[i];
                    assert!(x>=left && x+widths[i]<=right);
                    if i>0 && row==positions[i-1].1 {assert!(x>=positions[i-1].0+widths[i-1]+s(8));}
                }
                assert_eq!(positions[2],(right-widths[2],last));
            }
        }
    }
    #[test]fn english_shortcuts_keep_section_and_key_columns(){
        let _language=zsdesk_core::i18n::Scope::english(true);
        let rows=shortcut_rows(crate::window::shortcut_help_text());
        assert_eq!(rows.iter().filter(|row|row.section).count(),if crate::edition::DESKTOP&&!crate::edition::DESKTOP_ONLY{2}else{1});
        if !crate::edition::DESKTOP_ONLY{assert!(rows.iter().any(|row|row.key=="Ctrl+W / Ctrl+F4"&&row.description=="Close tab"));}
        if crate::edition::DESKTOP{assert!(rows.iter().any(|row|row.key=="Hover"&&row.description=="Expand a collapsed fence"));}
    }
    #[test]fn shortcut_aliases_and_groups_keep_their_meaning(){
        let rows=shortcut_rows("文件管理器\nCtrl+W / Ctrl+F4 关闭标签　Ctrl+F1 展开或收起命令栏\n\n桌面格子\n鼠标悬停 展开格子");
        assert_eq!(rows.len(),5);assert!(rows[0].section&&rows[3].section);
        assert_eq!(rows[1].key,"Ctrl+W / Ctrl+F4");assert_eq!(rows[1].description,"关闭标签");
        assert_eq!(rows[4].key,"鼠标悬停");assert_eq!(rows[4].description,"展开格子");
    }
}
