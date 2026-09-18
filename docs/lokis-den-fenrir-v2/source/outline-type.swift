import Foundation
import CoreText
import CoreGraphics
func outline(_ string: String, fontName: String, size: CGFloat, tracking: CGFloat) -> String {
 let font=CTFontCreateWithName(fontName as CFString,size,nil)
 let attrs: [NSAttributedString.Key:Any] = [NSAttributedString.Key(kCTFontAttributeName as String):font,NSAttributedString.Key(kCTKernAttributeName as String):tracking]
 let line=CTLineCreateWithAttributedString(NSAttributedString(string:string,attributes:attrs))
 var output=""
 for run in CTLineGetGlyphRuns(line) as! [CTRun] {
  let count=CTRunGetGlyphCount(run)
  var glyphs=[CGGlyph](repeating:0,count:count),positions=[CGPoint](repeating:.zero,count:count)
  CTRunGetGlyphs(run,CFRange(location:0,length:0),&glyphs);CTRunGetPositions(run,CFRange(location:0,length:0),&positions)
  for i in 0..<count {
   guard let path=CTFontCreatePathForGlyph(font,glyphs[i],nil) else {continue}
   var data=""
   func point(_ p:CGPoint)->String {String(format:"%.3f %.3f",Double(p.x+positions[i].x),Double(p.y+positions[i].y))}
   path.applyWithBlock { ptr in
    let e=ptr.pointee
    switch e.type {
    case .moveToPoint:data+="M"+point(e.points[0])
    case .addLineToPoint:data+="L"+point(e.points[0])
    case .addQuadCurveToPoint:data+="Q"+point(e.points[0])+" "+point(e.points[1])
    case .addCurveToPoint:data+="C"+point(e.points[0])+" "+point(e.points[1])+" "+point(e.points[2])
    case .closeSubpath:data+="Z"
    @unknown default:break
    }
   }
   output+="<path d=\"\(data)\"/>"
  }
 }
 return output
}
let result=["wordmark":outline("Loki’s Den",fontName:"Georgia",size:62,tracking:-2),"lockup":outline("Loki’s Den",fontName:"Georgia",size:58,tracking:-2),"endorsement":outline("BY LOKI’S LAB",fontName:"Arial",size:16,tracking:1.5)]
let data=try JSONSerialization.data(withJSONObject:result,options:[.prettyPrinted,.sortedKeys])
try data.write(to:URL(fileURLWithPath:"/tmp/den-outlines.json"))
