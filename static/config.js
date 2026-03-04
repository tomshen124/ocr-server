const ImageConfig = {
  basePath: './images/',
  
  materials: {
    '公司变更登记申请书': '智能预审_审核依据材料1.3.png',
    '质量担当合同': '智能预审_有审查点1.3.png',
    '建筑执照': '预审通过1.3.png',
    '公司章程': '智能预审_已通过材料1.3.png',
    '无审核依据': '智能预审_无审核依据材料1.3.png',
    '异常提示': '智能预审异常提示1.3.png'
  },
  
  getImagePath: function(materialName) {
    const filename = this.materials[materialName] || '智能预审_审核依据材料1.3.png';
    return this.basePath + filename;
  }
};

if (typeof module !== 'undefined' && module.exports) {
  module.exports = ImageConfig;
}
